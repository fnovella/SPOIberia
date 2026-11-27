use crate::api::spo::{SpoClient, SpoValueList, SPO_JSON_ACCEPT, SPO_JSON_VERBOSE};
use crate::api::{check_json_response, send_with_retry, validate_guid};
use crate::models::ViewFormatterBackup;
use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::json;

pub const SHORTCUT_COMMAND_BAR_KEYS: &[&str] = &[
    "addShortcut",
    "addShortcutToOneDriveCommand",
];

pub const SYNC_BUTTON_COMMAND_BAR_KEYS: &[&str] = &["sync", "syncCommand"];

pub const SYNC_COMMAND_BAR_KEYS: &[&str] = &[
    "addShortcut",
    "addShortcutToOneDriveCommand",
    "sync",
    "syncCommand",
];
const VIEW_FORMATTING_SCHEMA: &str =
    "https://developer.microsoft.com/json-schemas/sp/v2/row-formatting.schema.json";

#[derive(Debug, Clone)]
pub struct SpoLibraryView {
    pub id: String,
    pub title: String,
    pub default_view: bool,
    pub custom_formatter: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawListView {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Title")]
    title: Option<String>,
    #[serde(rename = "Hidden", default)]
    hidden: bool,
    #[serde(rename = "PersonalView", default)]
    personal_view: bool,
    #[serde(rename = "DefaultView", default)]
    default_view: bool,
    #[serde(rename = "CustomFormatter")]
    custom_formatter: Option<String>,
}

impl SpoClient {
    fn view_formatter_hides_any(custom_formatter: Option<&str>, keys: &[&str]) -> bool {
        let Some(raw) = custom_formatter.map(str::trim).filter(|s| !s.is_empty()) else {
            return false;
        };
        let Ok(val) = serde_json::from_str::<serde_json::Value>(raw) else {
            return false;
        };
        let Some(commands) = val
            .get("commandBarProps")
            .and_then(|p| p.get("commands"))
            .and_then(|c| c.as_array())
        else {
            return false;
        };
        commands.iter().any(|cmd| {
            let hide = cmd.get("hide").and_then(|v| v.as_bool()) == Some(true);
            let key = cmd.get("key").and_then(|v| v.as_str()).unwrap_or("");
            hide && keys.contains(&key)
        })
    }

    /// Return `true` when a view's `CustomFormatter` JSON hides `"addShortcut"` or `"addShortcutToOneDriveCommand"`.
    pub fn view_formatter_hides_shortcut(custom_formatter: Option<&str>) -> bool {
        Self::view_formatter_hides_any(custom_formatter, SHORTCUT_COMMAND_BAR_KEYS)
    }

    /// Return `true` when a view's `CustomFormatter` JSON hides `"sync"` or `"syncCommand"`.
    pub fn view_formatter_hides_sync(custom_formatter: Option<&str>) -> bool {
        Self::view_formatter_hides_any(custom_formatter, SYNC_BUTTON_COMMAND_BAR_KEYS)
    }

    /// Compute the updated `CustomFormatter` JSON to hide (`hide=true`) or restore
    /// (`hide=false`) specific command bar buttons (`target_keys`) while preserving
    /// any existing formatting rules. Returns `None` if no change is needed (or JSON is non-object).
    pub fn build_command_bar_view_formatter(
        existing: Option<&str>,
        target_keys: &[&str],
        hide: bool,
    ) -> Option<String> {
        let trimmed = existing.unwrap_or("").trim();

        if hide {
            if trimmed.is_empty() {
                let commands: Vec<serde_json::Value> = target_keys
                    .iter()
                    .map(|k| json!({ "key": k, "hide": true }))
                    .collect();
                let doc = json!({
                    "$schema": VIEW_FORMATTING_SCHEMA,
                    "commandBarProps": {
                        "commands": commands
                    }
                });
                return serde_json::to_string(&doc).ok();
            }

            let mut root: serde_json::Value = serde_json::from_str(trimmed).ok()?;
            let root_obj = root.as_object_mut()?;
            if !root_obj.contains_key("$schema") {
                root_obj.insert("$schema".to_string(), json!(VIEW_FORMATTING_SCHEMA));
            }
            let bar_obj = root_obj
                .entry("commandBarProps".to_string())
                .or_insert_with(|| json!({}))
                .as_object_mut()?;
            let cmds_arr = bar_obj
                .entry("commands".to_string())
                .or_insert_with(|| json!([]))
                .as_array_mut()?;

            let mut changed = false;
            for &target_key in target_keys {
                if let Some(existing_cmd) = cmds_arr.iter_mut().find(|item| {
                    item.get("key")
                        .and_then(|k| k.as_str())
                        .map(|s| s == target_key)
                        .unwrap_or(false)
                }) {
                    if let Some(cmd_obj) = existing_cmd.as_object_mut() {
                        if cmd_obj.get("hide").and_then(|v| v.as_bool()) != Some(true) {
                            cmd_obj.insert("hide".to_string(), json!(true));
                            changed = true;
                        }
                    }
                } else {
                    cmds_arr.push(json!({ "key": target_key, "hide": true }));
                    changed = true;
                }
            }

            if !changed {
                return None;
            }
            serde_json::to_string(&root).ok()
        } else {
            if trimmed.is_empty() {
                return None;
            }

            let mut root: serde_json::Value = serde_json::from_str(trimmed).ok()?;
            let root_obj = root.as_object_mut()?;
            let bar_obj = root_obj.get_mut("commandBarProps")?.as_object_mut()?;
            let cmds_arr = bar_obj.get_mut("commands")?.as_array_mut()?;

            let mut changed = false;
            let mut new_cmds = Vec::with_capacity(cmds_arr.len());
            for mut item in std::mem::take(cmds_arr) {
                let is_target_key = item
                    .get("key")
                    .and_then(|k| k.as_str())
                    .map(|k| target_keys.contains(&k))
                    .unwrap_or(false);
                if is_target_key {
                    if let Some(obj) = item.as_object_mut() {
                        let has_hide_true =
                            obj.get("hide").and_then(|v| v.as_bool()) == Some(true);
                        let only_key_and_hide = has_hide_true
                            && obj.len() <= 2
                            && obj.keys().all(|k| k == "key" || k == "hide");
                        if only_key_and_hide {
                            changed = true;
                            continue;
                        } else if obj.remove("hide").is_some() {
                            changed = true;
                        }
                    }
                }
                new_cmds.push(item);
            }

            if !changed {
                return None;
            }

            *cmds_arr = new_cmds;
            if cmds_arr.is_empty() {
                bar_obj.remove("commands");
            }
            if bar_obj.is_empty() {
                root_obj.remove("commandBarProps");
            }

            let only_schema_or_empty =
                root_obj.is_empty() || (root_obj.len() == 1 && root_obj.contains_key("$schema"));
            if only_schema_or_empty {
                Some(String::new())
            } else {
                serde_json::to_string(&root).ok()
            }
        }
    }

    /// Compute the updated `CustomFormatter` JSON to hide (`hide_shortcuts=true`) or restore
    /// (`hide_shortcuts=false`) OneDrive shortcut command bar buttons (`addShortcut`,
    /// `addShortcutToOneDriveCommand`) without touching SharePoint `sync` keys.
    pub fn build_shortcut_view_formatter(
        existing: Option<&str>,
        hide_shortcuts: bool,
    ) -> Option<String> {
        Self::build_command_bar_view_formatter(existing, SHORTCUT_COMMAND_BAR_KEYS, hide_shortcuts)
    }

    /// Compute the updated `CustomFormatter` JSON to hide (`hide_sync=true`) or restore
    /// (`hide_sync=false`) SharePoint sync command bar buttons (`sync`, `syncCommand`)
    /// without touching OneDrive shortcut keys.
    pub fn build_sync_button_view_formatter(
        existing: Option<&str>,
        hide_sync: bool,
    ) -> Option<String> {
        Self::build_command_bar_view_formatter(existing, SYNC_BUTTON_COMMAND_BAR_KEYS, hide_sync)
    }

    /// Legacy helper: compute the updated `CustomFormatter` JSON to hide or restore all sync & shortcut keys.
    pub fn build_sync_view_formatter(
        existing: Option<&str>,
        hide_sync_commands: bool,
    ) -> Option<String> {
        Self::build_command_bar_view_formatter(existing, SYNC_COMMAND_BAR_KEYS, hide_sync_commands)
    }

    /// List non-hidden, non-personal views of a Document Library with their `CustomFormatter`.
    pub async fn get_library_views(
        &self,
        site_url: &str,
        library_id: &str,
    ) -> Result<Vec<SpoLibraryView>> {
        let library_id = validate_guid(library_id)?;
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let url = format!(
            "{}/_api/web/lists(guid'{}')/views?$select=Id,Title,Hidden,PersonalView,DefaultView,CustomFormatter",
            base, library_id
        );

        let resp = send_with_retry(|| {
            self.http
                .get(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
        })
        .await?;

        let raw: SpoValueList<RawListView> = check_json_response(
            resp,
            &format!("Error listando vistas de biblioteca '{}'", library_id),
        )
        .await?;

        Ok(raw
            .value
            .into_iter()
            .filter(|v| !v.hidden && !v.personal_view)
            .filter_map(|v| {
                let title = v.title.unwrap_or_default();
                if title.trim().is_empty() {
                    return None;
                }
                Some(SpoLibraryView {
                    id: v.id,
                    title,
                    default_view: v.default_view,
                    custom_formatter: v.custom_formatter,
                })
            })
            .collect())
    }

    /// Update a view's `CustomFormatter` property (`POST /_api/web/lists(guid'...')/views(guid'...')` with `MERGE`).
    pub async fn set_view_custom_formatter(
        &self,
        site_url: &str,
        library_id: &str,
        view_id: &str,
        custom_formatter: &str,
    ) -> Result<()> {
        let library_id = validate_guid(library_id)?;
        let view_id = validate_guid(view_id)?;
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let url = format!(
            "{}/_api/web/lists(guid'{}')/views(guid'{}')",
            base, library_id, view_id
        );
        let cf_value = if custom_formatter.trim().is_empty() {
            serde_json::Value::Null
        } else {
            json!(custom_formatter)
        };
        let payload = serde_json::to_string(&json!({
            "__metadata": { "type": "SP.View" },
            "CustomFormatter": cf_value
        }))?;

        let resp = send_with_retry(|| {
            self.http
                .post(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_VERBOSE)
                .header("Content-Type", SPO_JSON_VERBOSE)
                .header("X-HTTP-Method", "MERGE")
                .header("If-Match", "*")
                .body(payload.clone())
        })
        .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err_txt = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Error actualizando CustomFormatter en vista '{}' de biblioteca '{}' (HTTP {}): {}",
                view_id,
                library_id,
                status,
                err_txt
            ));
        }
        Ok(())
    }

    async fn apply_library_command_bar_view_formatting(
        &self,
        site_url: &str,
        library_id: &str,
        target_keys: &[&str],
        hide: bool,
    ) -> Result<Vec<ViewFormatterBackup>> {
        let views = self.get_library_views(site_url, library_id).await?;
        let mut backups = Vec::new();
        for v in views {
            if let Some(new_fmt) =
                Self::build_command_bar_view_formatter(v.custom_formatter.as_deref(), target_keys, hide)
            {
                self.set_view_custom_formatter(site_url, library_id, &v.id, &new_fmt)
                    .await?;
                backups.push(ViewFormatterBackup {
                    view_id: v.id,
                    view_title: v.title,
                    previous_custom_formatter: v.custom_formatter.unwrap_or_default(),
                });
            }
        }
        Ok(backups)
    }

    /// Apply or remove OneDrive shortcut JSON View Formatting (`commandBarProps`) on all public views
    /// of a Document Library, returning a backup of every modified view for deterministic SQLite Rollback.
    pub async fn apply_library_shortcut_view_formatting(
        &self,
        site_url: &str,
        library_id: &str,
        hide_shortcuts: bool,
    ) -> Result<Vec<ViewFormatterBackup>> {
        self.apply_library_command_bar_view_formatting(
            site_url,
            library_id,
            SHORTCUT_COMMAND_BAR_KEYS,
            hide_shortcuts,
        )
        .await
    }

    /// Apply or remove SharePoint sync button JSON View Formatting (`sync`, `syncCommand`) on all
    /// public views of a Document Library without touching OneDrive shortcut keys.
    pub async fn apply_library_sync_button_view_formatting(
        &self,
        site_url: &str,
        library_id: &str,
        hide_sync: bool,
    ) -> Result<Vec<ViewFormatterBackup>> {
        self.apply_library_command_bar_view_formatting(
            site_url,
            library_id,
            SYNC_BUTTON_COMMAND_BAR_KEYS,
            hide_sync,
        )
        .await
    }

    /// Legacy helper: apply or remove JSON View Formatting (`commandBarProps`) on all public views of a Document Library,
    /// returning a backup of every modified view for deterministic SQLite Rollback.
    pub async fn apply_library_sync_view_formatting(
        &self,
        site_url: &str,
        library_id: &str,
        hide_sync_commands: bool,
    ) -> Result<Vec<ViewFormatterBackup>> {
        self.apply_library_command_bar_view_formatting(
            site_url,
            library_id,
            SYNC_COMMAND_BAR_KEYS,
            hide_sync_commands,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_sync_view_formatter_merge_idempotent_and_restore() {
        // 1. Fresh hide on empty CustomFormatter
        let created = SpoClient::build_sync_view_formatter(None, true).expect("created formatter");
        assert!(SpoClient::view_formatter_hides_shortcut(Some(&created)));
        for key in SYNC_COMMAND_BAR_KEYS {
            assert!(created.contains(key), "missing key {key} in {created}");
        }

        // 2. Idempotent second hide -> None
        assert_eq!(SpoClient::build_sync_view_formatter(Some(&created), true), None);

        // 3. Hide while preserving existing additionalRowClass and custom commandBarProps command
        let existing_custom = r#"{"additionalRowClass":"sp-field-severity--good","commandBarProps":{"commands":[{"key":"editInGridView","hide":true}]}}"#;
        let merged = SpoClient::build_sync_view_formatter(Some(existing_custom), true)
            .expect("merged formatter");
        let merged_val: serde_json::Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(
            merged_val.get("additionalRowClass").and_then(|v| v.as_str()),
            Some("sp-field-severity--good")
        );
        let cmds = merged_val["commandBarProps"]["commands"].as_array().unwrap();
        assert!(cmds
            .iter()
            .any(|c| c["key"] == "editInGridView" && c["hide"] == true));
        assert!(SpoClient::view_formatter_hides_shortcut(Some(&merged)));

        // 4. Unhide on formatter that only had our 4 keys -> clears to empty string ""
        let cleared = SpoClient::build_sync_view_formatter(Some(&created), false)
            .expect("cleared formatter");
        assert_eq!(cleared, "");

        // 5. Unhide on merged formatter -> strips only our 4 keys and keeps additionalRowClass + editInGridView
        let restored = SpoClient::build_sync_view_formatter(Some(&merged), false)
            .expect("restored formatter");
        assert!(!SpoClient::view_formatter_hides_shortcut(Some(&restored)));
        let restored_val: serde_json::Value = serde_json::from_str(&restored).unwrap();
        assert_eq!(
            restored_val.get("additionalRowClass").and_then(|v| v.as_str()),
            Some("sp-field-severity--good")
        );
        let restored_cmds = restored_val["commandBarProps"]["commands"].as_array().unwrap();
        assert_eq!(restored_cmds.len(), 1);
        assert_eq!(restored_cmds[0]["key"], "editInGridView");

        // 6. Unhide on empty formatter -> None
        assert_eq!(SpoClient::build_sync_view_formatter(None, false), None);
        assert_eq!(SpoClient::build_sync_view_formatter(Some(""), false), None);
    }

    #[test]
    fn test_build_shortcut_view_formatter_only_touches_shortcut_keys() {
        let created =
            SpoClient::build_shortcut_view_formatter(None, true).expect("created shortcut fmt");
        assert!(SpoClient::view_formatter_hides_shortcut(Some(&created)));
        for key in SHORTCUT_COMMAND_BAR_KEYS {
            assert!(created.contains(key), "missing shortcut key {key}");
        }
        assert!(!created.contains("\"sync\""));
        assert!(!created.contains("\"syncCommand\""));

        // Preserve existing syncCommand + editInGridView when hiding/unhiding shortcuts
        let existing = r#"{"additionalRowClass":"sp-field-severity--good","commandBarProps":{"commands":[{"key":"sync","hide":true},{"key":"editInGridView","hide":true}]}}"#;
        let merged = SpoClient::build_shortcut_view_formatter(Some(existing), true)
            .expect("merged shortcut fmt");
        assert!(SpoClient::view_formatter_hides_shortcut(Some(&merged)));

        let restored = SpoClient::build_shortcut_view_formatter(Some(&merged), false)
            .expect("restored shortcut fmt");
        assert!(!SpoClient::view_formatter_hides_shortcut(Some(&restored)));
        let val: serde_json::Value = serde_json::from_str(&restored).unwrap();
        let cmds = val["commandBarProps"]["commands"].as_array().unwrap();
        assert_eq!(cmds.len(), 2);
        assert!(cmds.iter().any(|c| c["key"] == "sync" && c["hide"] == true));
        assert!(cmds
            .iter()
            .any(|c| c["key"] == "editInGridView" && c["hide"] == true));
    }

    #[test]
    fn test_build_sync_button_view_formatter_only_touches_sync_keys() {
        let created =
            SpoClient::build_sync_button_view_formatter(None, true).expect("created sync fmt");
        assert!(SpoClient::view_formatter_hides_sync(Some(&created)));
        assert!(!SpoClient::view_formatter_hides_shortcut(Some(&created)));
        for key in SYNC_BUTTON_COMMAND_BAR_KEYS {
            assert!(created.contains(key), "missing sync key {key}");
        }
        assert!(!created.contains("addShortcut"));

        // Preserve existing addShortcut + addShortcutToOneDriveCommand when hiding/unhiding sync
        let existing = r#"{"additionalRowClass":"sp-field-severity--good","commandBarProps":{"commands":[{"key":"addShortcut","hide":true},{"key":"addShortcutToOneDriveCommand","hide":true}]}}"#;
        let merged = SpoClient::build_sync_button_view_formatter(Some(existing), true)
            .expect("merged sync fmt");
        assert!(SpoClient::view_formatter_hides_sync(Some(&merged)));
        assert!(SpoClient::view_formatter_hides_shortcut(Some(&merged)));

        let restored = SpoClient::build_sync_button_view_formatter(Some(&merged), false)
            .expect("restored sync fmt");
        assert!(!SpoClient::view_formatter_hides_sync(Some(&restored)));
        assert!(SpoClient::view_formatter_hides_shortcut(Some(&restored)));
        let val: serde_json::Value = serde_json::from_str(&restored).unwrap();
        let cmds = val["commandBarProps"]["commands"].as_array().unwrap();
        assert_eq!(cmds.len(), 2);
        assert!(cmds
            .iter()
            .any(|c| c["key"] == "addShortcut" && c["hide"] == true));
        assert!(cmds
            .iter()
            .any(|c| c["key"] == "addShortcutToOneDriveCommand" && c["hide"] == true));
    }
}
