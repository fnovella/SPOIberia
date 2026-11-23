use crate::api::spo::SpoClient;
use crate::api::{check_json_response, send_with_retry};
use crate::models::{
    FolderPermissionEntry, SpoFolder, SpoLibrary, SpoRoleDefinition, ViewFormatterBackup,
};
use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::json;

const SPO_JSON_ACCEPT: &str = "application/json;odata=nometadata";
const SPO_JSON_CONTENT: &str = "application/json;odata=nometadata";
const SPO_JSON_VERBOSE: &str = "application/json;odata=verbose";

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

const SYSTEM_LIBRARY_TITLES: &[&str] = &[
    "Style Library",
    "Biblioteca de estilos",
    "Site Assets",
    "Activos del sitio",
    "Site Pages",
    "Páginas del sitio",
    "Form Templates",
    "Plantillas de formulario",
    "Preservation Hold Library",
    "Biblioteca de conservación",
    "Biblioteca de retención",
    "Site Collection Documents",
    "Documentos de colección de sitios",
    "Documentos de la colección de sitios",
    "Site Collection Images",
    "Imágenes de colección de sitios",
    "Imágenes de la colección de sitios",
];

const SYSTEM_LIBRARY_URL_SEGMENTS: &[&str] = &[
    "Style Library",
    "SiteAssets",
    "SitePages",
    "FormServerTemplates",
    "PreservationHoldLibrary",
    "SiteCollectionDocuments",
    "SiteCollectionImages",
    "ConvertedForms",
];

#[derive(Debug, Deserialize)]
struct SpoValueList<T> {
    value: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct RawList {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Title")]
    title: String,
    #[serde(rename = "BaseTemplate")]
    base_template: i32,
    #[serde(rename = "Hidden", default)]
    hidden: bool,
    #[serde(rename = "ItemCount", default)]
    item_count: i64,
    #[serde(rename = "ExcludeFromOfflineClient", default)]
    exclude_from_offline_client: bool,
    #[serde(rename = "RootFolder")]
    root_folder: Option<RawRootFolder>,
}

#[derive(Debug, Deserialize)]
struct RawOfflineSyncFlag {
    #[serde(rename = "ExcludeFromOfflineClient", default)]
    exclude_from_offline_client: bool,
}

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

#[derive(Debug, Deserialize)]
struct RawRootFolder {
    #[serde(rename = "ServerRelativeUrl")]
    server_relative_url: String,
}

#[derive(Debug, Deserialize)]
struct RawFolder {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "ServerRelativeUrl")]
    server_relative_url: String,
    #[serde(rename = "ListItemAllFields")]
    list_item_all_fields: Option<RawFolderListItem>,
}

#[derive(Debug, Deserialize)]
struct RawFolderListItem {
    #[serde(rename = "Id")]
    id: Option<i64>,
    #[serde(rename = "HasUniqueRoleAssignments", default)]
    has_unique_role_assignments: bool,
}

#[derive(Debug, Deserialize)]
struct RawRoleDefinition {
    #[serde(rename = "Id")]
    id: i64,
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Description")]
    description: Option<String>,
    #[serde(rename = "RoleTypeKind", default)]
    role_type_kind: i32,
}

#[derive(Debug, Deserialize)]
struct RawItemRoleAssignment {
    #[serde(rename = "Member")]
    member: Option<RawPrincipalMember>,
    #[serde(rename = "RoleDefinitionBindings", default)]
    role_definition_bindings: Vec<RawRoleDefName>,
}

#[derive(Debug, Deserialize)]
struct RawPrincipalMember {
    #[serde(rename = "Id")]
    id: i64,
    #[serde(rename = "Title")]
    title: String,
    #[serde(rename = "LoginName")]
    login_name: Option<String>,
    #[serde(rename = "PrincipalType", default)]
    principal_type: i32,
}

#[derive(Debug, Deserialize)]
struct RawRoleDefName {
    #[serde(rename = "Name")]
    name: String,
}

impl SpoClient {
    /// Detect system/template SharePoint libraries across languages via title and language-independent URL segment.
    pub(crate) fn is_system_library(title: &str, server_relative_url: &str) -> bool {
        let trimmed_url = server_relative_url.trim_end_matches('/');
        let lower_url = trimmed_url.to_lowercase();
        if lower_url.ends_with("/_catalogs") || lower_url.contains("/_catalogs/") {
            return true;
        }
        let clean_title = title.trim();
        if SYSTEM_LIBRARY_TITLES
            .iter()
            .any(|sys| sys.eq_ignore_ascii_case(clean_title))
        {
            return true;
        }
        let last_segment = trimmed_url.rsplit('/').next().unwrap_or("");
        SYSTEM_LIBRARY_URL_SEGMENTS
            .iter()
            .any(|seg| seg.eq_ignore_ascii_case(last_segment))
    }

    /// List non-system Document Libraries (`BaseTemplate == 101`) in a site.
    pub async fn get_document_libraries(&self, site_url: &str) -> Result<Vec<SpoLibrary>> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let url = format!(
            "{}/_api/web/lists?$select=Id,Title,BaseTemplate,Hidden,ItemCount,ExcludeFromOfflineClient,RootFolder/ServerRelativeUrl&$expand=RootFolder&$filter=BaseTemplate eq 101 and Hidden eq false",
            base
        );

        let resp = send_with_retry(|| {
            self.http
                .get(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
        })
        .await?;

        let raw: SpoValueList<RawList> =
            check_json_response(resp, "Error listando bibliotecas del sitio").await?;

        let mut libs: Vec<SpoLibrary> = raw
            .value
            .into_iter()
            .filter(|l| l.base_template == 101 && !l.hidden)
            .filter_map(|l| {
                let srv_url = l.root_folder?.server_relative_url;
                if Self::is_system_library(&l.title, &srv_url) {
                    return None;
                }
                let is_default = srv_url.ends_with("/Shared Documents")
                    || l.title.eq_ignore_ascii_case("Documents")
                    || l.title.eq_ignore_ascii_case("Documentos")
                    || l.title.eq_ignore_ascii_case("Documentos compartidos");
                Some(SpoLibrary {
                    id: l.id,
                    title: l.title,
                    server_relative_url: srv_url,
                    item_count: l.item_count,
                    is_default_documents: is_default,
                    exclude_from_offline_client: l.exclude_from_offline_client,
                    views_total: 0,
                    views_hiding_shortcut: 0,
                    views_hiding_sync: 0,
                    shortcut_button_hidden: false,
                })
            })
            .collect();

        libs.sort_by(|a, b| {
            b.is_default_documents
                .cmp(&a.is_default_documents)
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        });
        Ok(libs)
    }

    /// Resolve a library by title, or automatically map "Documents"/"Documentos" to the site's default Documents library.
    pub async fn resolve_library(&self, site_url: &str, library_name: &str) -> Result<SpoLibrary> {
        let libs = self.get_document_libraries(site_url).await?;
        let trimmed = library_name.trim();
        let is_generic_docs = trimmed.is_empty()
            || trimmed.eq_ignore_ascii_case("Documents")
            || trimmed.eq_ignore_ascii_case("Documentos")
            || trimmed.eq_ignore_ascii_case("Shared Documents")
            || trimmed.eq_ignore_ascii_case("Documentos compartidos");

        if let Some(exact) = libs.iter().find(|l| l.title.eq_ignore_ascii_case(trimmed)) {
            return Ok(exact.clone());
        }

        if is_generic_docs {
            if let Some(def) = libs
                .iter()
                .find(|l| l.is_default_documents)
                .or_else(|| libs.first())
            {
                return Ok(def.clone());
            }
        }

        Err(anyhow!(
            "No se encontró la biblioteca '{}' en '{}'",
            library_name,
            site_url
        ))
    }

    /// Create a new Document Library (`BaseTemplate: 101`) in the site.
    pub async fn create_document_library(
        &self,
        site_url: &str,
        title: &str,
        description: &str,
    ) -> Result<SpoLibrary> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let url = format!("{}/_api/web/lists", base);
        let body = json!({
            "BaseTemplate": 101,
            "Title": title,
            "Description": description
        });

        let resp = send_with_retry(|| {
            self.http
                .post(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
                .header("Content-Type", SPO_JSON_CONTENT)
                .json(&body)
        })
        .await?;

        if !resp.status().is_success() {
            let err_txt = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Error creando biblioteca '{}': {}",
                title,
                err_txt
            ));
        }

        self.resolve_library(site_url, title).await
    }

    pub async fn delete_document_library(&self, site_url: &str, library_id: &str) -> Result<()> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let url = format!("{}/_api/web/lists(guid'{}')", base, library_id);

        let resp = send_with_retry(|| {
            self.http
                .delete(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
                .header("X-HTTP-Method", "DELETE")
                .header("If-Match", "*")
        })
        .await?;

        if !resp.status().is_success() {
            let err_txt = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Error eliminando biblioteca {}: {}",
                library_id,
                err_txt
            ));
        }
        Ok(())
    }

    /// Read the current `ExcludeFromOfflineClient` flag of a Document Library.
    pub async fn get_library_offline_sync(
        &self,
        site_url: &str,
        library_id: &str,
    ) -> Result<bool> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let url = format!(
            "{}/_api/web/lists(guid'{}')?$select=ExcludeFromOfflineClient",
            base, library_id
        );

        let resp = send_with_retry(|| {
            self.http
                .get(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
        })
        .await?;

        let raw: RawOfflineSyncFlag = check_json_response(
            resp,
            &format!("Error leyendo ExcludeFromOfflineClient de biblioteca '{}'", library_id),
        )
        .await?;
        Ok(raw.exclude_from_offline_client)
    }

    /// Enable (`exclude=false`) or disable (`exclude=true`) offline sync / OneDrive shortcuts for a Document Library.
    pub async fn set_library_offline_sync(
        &self,
        site_url: &str,
        library_id: &str,
        exclude: bool,
    ) -> Result<()> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let url = format!("{}/_api/web/lists(guid'{}')", base, library_id);
        let payload = serde_json::to_string(&json!({
            "__metadata": { "type": "SP.List" },
            "ExcludeFromOfflineClient": exclude
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
                "Error actualizando ExcludeFromOfflineClient={} en biblioteca '{}' (HTTP {}): {}",
                exclude,
                library_id,
                status,
                err_txt
            ));
        }
        Ok(())
    }

    /// Return `true` when a view's `CustomFormatter` JSON hides `"addShortcut"` or `"addShortcutToOneDriveCommand"`.
    pub fn view_formatter_hides_shortcut(custom_formatter: Option<&str>) -> bool {
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
            hide && (key == "addShortcut" || key == "addShortcutToOneDriveCommand")
        })
    }

    /// Return `true` when a view's `CustomFormatter` JSON hides `"sync"` or `"syncCommand"`.
    pub fn view_formatter_hides_sync(custom_formatter: Option<&str>) -> bool {
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
            hide && (key == "sync" || key == "syncCommand")
        })
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
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let url = format!(
            "{}/_api/web/lists(guid'{}')/views(guid'{}')",
            base, library_id, view_id
        );
        let payload = serde_json::to_string(&json!({
            "__metadata": { "type": "SP.View" },
            "CustomFormatter": custom_formatter
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

    /// Apply or remove OneDrive shortcut JSON View Formatting (`commandBarProps`) on all public views
    /// of a Document Library, returning a backup of every modified view for deterministic SQLite Rollback.
    pub async fn apply_library_shortcut_view_formatting(
        &self,
        site_url: &str,
        library_id: &str,
        hide_shortcuts: bool,
    ) -> Result<Vec<ViewFormatterBackup>> {
        let views = self.get_library_views(site_url, library_id).await?;
        let mut backups = Vec::new();
        for v in views {
            if let Some(new_fmt) =
                Self::build_shortcut_view_formatter(v.custom_formatter.as_deref(), hide_shortcuts)
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

    /// Apply or remove SharePoint sync button JSON View Formatting (`sync`, `syncCommand`) on all
    /// public views of a Document Library without touching OneDrive shortcut keys.
    pub async fn apply_library_sync_button_view_formatting(
        &self,
        site_url: &str,
        library_id: &str,
        hide_sync: bool,
    ) -> Result<Vec<ViewFormatterBackup>> {
        let views = self.get_library_views(site_url, library_id).await?;
        let mut backups = Vec::new();
        for v in views {
            if let Some(new_fmt) =
                Self::build_sync_button_view_formatter(v.custom_formatter.as_deref(), hide_sync)
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

    /// Legacy helper: apply or remove JSON View Formatting (`commandBarProps`) on all public views of a Document Library,
    /// returning a backup of every modified view for deterministic SQLite Rollback.
    pub async fn apply_library_sync_view_formatting(
        &self,
        site_url: &str,
        library_id: &str,
        hide_sync_commands: bool,
    ) -> Result<Vec<ViewFormatterBackup>> {
        let views = self.get_library_views(site_url, library_id).await?;
        let mut backups = Vec::new();
        for v in views {
            if let Some(new_fmt) =
                Self::build_sync_view_formatter(v.custom_formatter.as_deref(), hide_sync_commands)
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

    /// List folders inside a library (or inside a subfolder relative path) along with their permission status.
    pub async fn list_folders(
        &self,
        site_url: &str,
        library_name: &str,
        relative_subpath: Option<&str>,
    ) -> Result<Vec<SpoFolder>> {
        let lib = self.resolve_library(site_url, library_name).await?;
        let lib_root = lib.server_relative_url.trim_end_matches('/');
        let target_folder_url = match relative_subpath.map(|s| s.trim_matches('/')).filter(|s| !s.is_empty()) {
            Some(sub) => format!("{}/{}", lib_root, sub),
            None => lib_root.to_string(),
        };

        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let escaped_path = target_folder_url.replace('\'', "''");
        let url = format!(
            "{}/_api/web/GetFolderByServerRelativePath(decodedurl='{}')/Folders?$select=Name,ServerRelativeUrl,ListItemAllFields/Id,ListItemAllFields/HasUniqueRoleAssignments&$expand=ListItemAllFields",
            base,
            urlencoding::encode(&escaped_path)
        );

        let resp = send_with_retry(|| {
            self.http
                .get(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
        })
        .await?;

        let raw: SpoValueList<RawFolder> =
            check_json_response(resp, "Error listando carpetas").await?;

        let mut out = Vec::new();
        for f in raw.value {
            if f.name.eq_ignore_ascii_case("Forms") || f.name.starts_with('_') {
                continue;
            }
            let rel = f
                .server_relative_url
                .strip_prefix(lib_root)
                .unwrap_or(&f.server_relative_url)
                .trim_start_matches('/')
                .to_string();

            let item_id = f.list_item_all_fields.as_ref().and_then(|li| li.id);
            let has_unique = f
                .list_item_all_fields
                .as_ref()
                .map(|li| li.has_unique_role_assignments)
                .unwrap_or(false);

            let role_assignments = if has_unique {
                self.get_folder_role_assignments(site_url, &f.server_relative_url)
                    .await
                    .unwrap_or_default()
            } else {
                Vec::new()
            };

            out.push(SpoFolder {
                name: f.name,
                server_relative_url: f.server_relative_url,
                relative_path: rel,
                item_id,
                has_unique_role_assignments: has_unique,
                role_assignments,
            });
        }

        out.sort_by_key(|a| a.name.to_lowercase());
        Ok(out)
    }

    /// Create a folder path hierarchically (creating parent segments top-down if needed).
    /// Returns `(final_server_relative_url, created_new)`.
    pub async fn ensure_folder_path(
        &self,
        site_url: &str,
        library_name: &str,
        relative_folder_path: &str,
    ) -> Result<(String, bool)> {
        let lib = self.resolve_library(site_url, library_name).await?;
        let lib_root = lib.server_relative_url.trim_end_matches('/');
        let clean_rel = relative_folder_path.trim_matches('/');
        if clean_rel.is_empty() {
            return Ok((lib_root.to_string(), false));
        }

        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;

        let mut current_path = lib_root.to_string();
        let mut created_leaf = false;

        for segment in clean_rel.split('/').filter(|s| !s.is_empty()) {
            current_path = format!("{}/{}", current_path, segment);
            let exists = self.folder_exists(site_url, &current_path).await.unwrap_or(false);
            if !exists {
                let escaped = current_path.replace('\'', "''");
                let add_url = format!(
                    "{}/_api/web/folders/addUsingPath(DecodedUrl='{}',overwrite=false)",
                    base,
                    urlencoding::encode(&escaped)
                );
                let resp = send_with_retry(|| {
                    self.http
                        .post(&add_url)
                        .bearer_auth(&token)
                        .header("Accept", SPO_JSON_ACCEPT)
                        .header("Content-Length", "0")
                })
                .await?;
                if !resp.status().is_success() {
                    let err_txt = resp.text().await.unwrap_or_default();
                    return Err(anyhow!(
                        "Error creando carpeta '{}': {}",
                        current_path,
                        err_txt
                    ));
                }
                created_leaf = true;
            }
        }

        Ok((current_path, created_leaf))
    }

    pub async fn folder_exists(&self, site_url: &str, server_relative_url: &str) -> Result<bool> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let escaped = server_relative_url.replace('\'', "''");
        let url = format!(
            "{}/_api/web/GetFolderByServerRelativePath(decodedurl='{}')?$select=Exists",
            base,
            urlencoding::encode(&escaped)
        );

        let resp = send_with_retry(|| {
            self.http
                .get(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
        })
        .await?;

        if !resp.status().is_success() {
            return Ok(false);
        }
        let val: serde_json::Value = resp.json().await?;
        Ok(val.get("Exists").and_then(|v| v.as_bool()).unwrap_or(true))
    }

    pub async fn delete_folder(&self, site_url: &str, server_relative_url: &str) -> Result<()> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let escaped = server_relative_url.replace('\'', "''");
        let url = format!(
            "{}/_api/web/GetFolderByServerRelativePath(decodedurl='{}')",
            base,
            urlencoding::encode(&escaped)
        );

        let resp = send_with_retry(|| {
            self.http
                .delete(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
                .header("X-HTTP-Method", "DELETE")
                .header("If-Match", "*")
        })
        .await?;

        if !resp.status().is_success() {
            let err_txt = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Error eliminando carpeta '{}': {}",
                server_relative_url,
                err_txt
            ));
        }
        Ok(())
    }

    /// Check if a folder currently has unique role assignments (`HasUniqueRoleAssignments`).
    pub async fn folder_has_unique_roles(
        &self,
        site_url: &str,
        server_relative_url: &str,
    ) -> Result<bool> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let escaped = server_relative_url.replace('\'', "''");
        let url = format!(
            "{}/_api/web/GetFolderByServerRelativePath(decodedurl='{}')/ListItemAllFields?$select=HasUniqueRoleAssignments",
            base,
            urlencoding::encode(&escaped)
        );

        let resp = send_with_retry(|| {
            self.http
                .get(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
        })
        .await?;

        if !resp.status().is_success() {
            return Ok(false);
        }
        let val: serde_json::Value = resp.json().await?;
        Ok(val
            .get("HasUniqueRoleAssignments")
            .and_then(|v| v.as_bool())
            .unwrap_or(false))
    }

    /// Get detailed role assignments for a specific folder or file server-relative URL.
    pub async fn get_folder_role_assignments(
        &self,
        site_url: &str,
        server_relative_url: &str,
    ) -> Result<Vec<FolderPermissionEntry>> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let escaped = server_relative_url.replace('\'', "''");
        let url = format!(
            "{}/_api/web/GetFolderByServerRelativePath(decodedurl='{}')/ListItemAllFields/RoleAssignments?$expand=Member,RoleDefinitionBindings",
            base,
            urlencoding::encode(&escaped)
        );

        let resp = send_with_retry(|| {
            self.http
                .get(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
        })
        .await?;

        if !resp.status().is_success() {
            return Ok(Vec::new());
        }

        let list: SpoValueList<RawItemRoleAssignment> = resp.json().await?;
        Ok(Self::parse_item_role_assignments(list.value))
    }

    fn parse_item_role_assignments(
        raw_list: Vec<RawItemRoleAssignment>,
    ) -> Vec<FolderPermissionEntry> {
        let mut out = Vec::new();
        for ra in raw_list {
            if let Some(member) = ra.member {
                if member.title.starts_with("SharingLinks") {
                    continue;
                }
                let roles: Vec<String> = ra
                    .role_definition_bindings
                    .into_iter()
                    .map(|r| r.name)
                    .filter(|n| n != "Limited Access" && n != "Acceso limitado")
                    .collect();
                if roles.is_empty() {
                    continue;
                }
                let p_type = match member.principal_type {
                    8 => "SharePointGroup",
                    4 => "SecurityGroup",
                    _ => "User",
                }
                .to_string();
                out.push(FolderPermissionEntry {
                    principal_id: member.id,
                    principal_title: member.title.clone(),
                    principal_login_name: member.login_name.unwrap_or(member.title),
                    principal_type: p_type,
                    roles,
                });
            }
        }
        out
    }

    /// Break role inheritance on a folder (`ListItemAllFields/breakroleinheritance`).
    pub async fn break_folder_inheritance(
        &self,
        site_url: &str,
        server_relative_url: &str,
        copy_role_assignments: bool,
        clear_subscopes: bool,
    ) -> Result<()> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let escaped = server_relative_url.replace('\'', "''");
        let url = format!(
            "{}/_api/web/GetFolderByServerRelativePath(decodedurl='{}')/ListItemAllFields/breakroleinheritance(copyRoleAssignments={},clearSubscopes={})",
            base,
            urlencoding::encode(&escaped),
            copy_role_assignments,
            clear_subscopes
        );

        let resp = send_with_retry(|| {
            self.http
                .post(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
                .header("Content-Length", "0")
        })
        .await?;

        if !resp.status().is_success() {
            let err_txt = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Error rompiendo herencia en '{}': {}",
                server_relative_url,
                err_txt
            ));
        }
        Ok(())
    }

    /// Reset role inheritance on a folder (`ListItemAllFields/resetroleinheritance`).
    pub async fn reset_folder_inheritance(
        &self,
        site_url: &str,
        server_relative_url: &str,
    ) -> Result<()> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let escaped = server_relative_url.replace('\'', "''");
        let url = format!(
            "{}/_api/web/GetFolderByServerRelativePath(decodedurl='{}')/ListItemAllFields/resetroleinheritance",
            base,
            urlencoding::encode(&escaped)
        );

        let resp = send_with_retry(|| {
            self.http
                .post(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
                .header("Content-Length", "0")
        })
        .await?;

        if !resp.status().is_success() {
            let err_txt = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Error restaurando herencia en '{}': {}",
                server_relative_url,
                err_txt
            ));
        }
        Ok(())
    }

    /// Retrieve all Role Definitions (Permission Levels) in the site.
    pub async fn get_role_definitions(&self, site_url: &str) -> Result<Vec<SpoRoleDefinition>> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let url = format!(
            "{}/_api/web/roledefinitions?$select=Id,Name,Description,RoleTypeKind",
            base
        );

        let resp = send_with_retry(|| {
            self.http
                .get(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
        })
        .await?;

        let raw: SpoValueList<RawRoleDefinition> =
            check_json_response(resp, "Error obteniendo RoleDefinitions del sitio").await?;

        Ok(raw
            .value
            .into_iter()
            .map(|r| SpoRoleDefinition {
                id: r.id,
                name: r.name,
                description: r.description.unwrap_or_default(),
                role_type_kind: r.role_type_kind,
            })
            .collect())
    }

    /// Language-independent Role Definition resolver: matches exact name or maps common English/Spanish names to `RoleTypeKind`.
    pub async fn resolve_role_definition(
        &self,
        site_url: &str,
        role_name: &str,
    ) -> Result<SpoRoleDefinition> {
        let defs = self.get_role_definitions(site_url).await?;
        let clean = role_name.trim();

        if let Some(exact) = defs.iter().find(|d| d.name.eq_ignore_ascii_case(clean)) {
            return Ok(exact.clone());
        }

        let target_kind = Self::role_name_to_kind(clean);
        if let Some(kind) = target_kind {
            if let Some(found) = defs.iter().find(|d| d.role_type_kind == kind) {
                return Ok(found.clone());
            }
        }

        Err(anyhow!(
            "No se encontró el nivel de permiso '{}' en '{}'",
            role_name,
            site_url
        ))
    }

    pub fn role_name_to_kind(role_name: &str) -> Option<i32> {
        match role_name.trim().to_lowercase().as_str() {
            "read" | "leer" | "lectura" | "reader" => Some(2),
            "contribute" | "colaborar" | "colaboración" | "contributor" => Some(3),
            "edit" | "editar" | "edición" | "edicion" | "design" | "diseño" => Some(4),
            "full control" | "fullcontrol" | "control total" | "admin" | "administrator" => Some(5),
            _ => None,
        }
    }

    /// Grant a Role Definition (`role_def_id`) to a Principal (`principal_id`) on a folder.
    pub async fn add_folder_role_assignment(
        &self,
        site_url: &str,
        server_relative_url: &str,
        principal_id: i64,
        role_def_id: i64,
    ) -> Result<()> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let escaped = server_relative_url.replace('\'', "''");
        let url = format!(
            "{}/_api/web/GetFolderByServerRelativePath(decodedurl='{}')/ListItemAllFields/roleassignments/addroleassignment(principalid={},roledefid={})",
            base,
            urlencoding::encode(&escaped),
            principal_id,
            role_def_id
        );

        let resp = send_with_retry(|| {
            self.http
                .post(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
                .header("Content-Length", "0")
        })
        .await?;

        if !resp.status().is_success() {
            let err_txt = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Error asignando rol {} al principal {} en '{}': {}",
                role_def_id,
                principal_id,
                server_relative_url,
                err_txt
            ));
        }
        Ok(())
    }

    /// Remove a Role Definition (`role_def_id`) from a Principal (`principal_id`) on a folder.
    pub async fn remove_folder_role_assignment(
        &self,
        site_url: &str,
        server_relative_url: &str,
        principal_id: i64,
        role_def_id: i64,
    ) -> Result<()> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let escaped = server_relative_url.replace('\'', "''");
        let url = format!(
            "{}/_api/web/GetFolderByServerRelativePath(decodedurl='{}')/ListItemAllFields/roleassignments/removeroleassignment(principalid={},roledefid={})",
            base,
            urlencoding::encode(&escaped),
            principal_id,
            role_def_id
        );

        let resp = send_with_retry(|| {
            self.http
                .post(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
                .header("Content-Length", "0")
        })
        .await?;

        if !resp.status().is_success() {
            let err_txt = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Error removiendo rol {} del principal {} en '{}': {}",
                role_def_id,
                principal_id,
                server_relative_url,
                err_txt
            ));
        }
        Ok(())
    }

    /// Grant a Role Definition to a Principal at the site (web) level.
    pub async fn add_site_role_assignment(
        &self,
        site_url: &str,
        principal_id: i64,
        role_def_id: i64,
    ) -> Result<()> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let url = format!(
            "{}/_api/web/roleassignments/addroleassignment(principalid={},roledefid={})",
            base, principal_id, role_def_id
        );

        let resp = send_with_retry(|| {
            self.http
                .post(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
                .header("Content-Length", "0")
        })
        .await?;

        if !resp.status().is_success() {
            let err_txt = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Error asignando rol de sitio {} al principal {}: {}",
                role_def_id,
                principal_id,
                err_txt
            ));
        }
        Ok(())
    }

    /// Remove a Role Definition from a Principal at the site (web) level.
    pub async fn remove_site_role_assignment(
        &self,
        site_url: &str,
        principal_id: i64,
        role_def_id: i64,
    ) -> Result<()> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let url = format!(
            "{}/_api/web/roleassignments/removeroleassignment(principalid={},roledefid={})",
            base, principal_id, role_def_id
        );

        let resp = send_with_retry(|| {
            self.http
                .post(&url)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
                .header("Content-Length", "0")
        })
        .await?;

        if !resp.status().is_success() {
            let err_txt = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Error removiendo rol de sitio {} del principal {}: {}",
                role_def_id,
                principal_id,
                err_txt
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_independent_role_mapping() {
        assert_eq!(SpoClient::role_name_to_kind("Read"), Some(2));
        assert_eq!(SpoClient::role_name_to_kind("Leer"), Some(2));
        assert_eq!(SpoClient::role_name_to_kind("Contribute"), Some(3));
        assert_eq!(SpoClient::role_name_to_kind("Colaborar"), Some(3));
        assert_eq!(SpoClient::role_name_to_kind("Edit"), Some(4));
        assert_eq!(SpoClient::role_name_to_kind("Editar"), Some(4));
        assert_eq!(SpoClient::role_name_to_kind("Full Control"), Some(5));
        assert_eq!(SpoClient::role_name_to_kind("Control total"), Some(5));
    }

    #[test]
    fn test_is_system_library_multilingual_and_url_based() {
        assert!(SpoClient::is_system_library(
            "Biblioteca de estilos",
            "/sites/Obra1/Style Library"
        ));
        assert!(SpoClient::is_system_library(
            "Formatbibliothek",
            "/sites/Obra1/Style Library/"
        ));
        assert!(SpoClient::is_system_library(
            "Recursos del sitio",
            "/sites/Obra1/SiteAssets"
        ));
        assert!(SpoClient::is_system_library(
            "Biblioteca de conservación",
            "/sites/Obra1/PreservationHoldLibrary"
        ));
        assert!(SpoClient::is_system_library(
            "Galería de páginas maestras",
            "/sites/Obra1/_catalogs/masterpage"
        ));

        assert!(!SpoClient::is_system_library(
            "Documentos",
            "/sites/Obra1/Shared Documents"
        ));
        assert!(!SpoClient::is_system_library(
            "Documents",
            "/sites/Obra1/Shared Documents"
        ));
        assert!(!SpoClient::is_system_library(
            "Planos de Obra",
            "/sites/Obra1/PlanosDeObra"
        ));
    }

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
