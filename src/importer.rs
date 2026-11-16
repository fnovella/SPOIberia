use crate::api::spo::SpoClient;
use crate::models::{MatrixFolderSpec, MatrixPermissionGrant, StagingOp};
use crate::staging::StagingQueue;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(non_snake_case)]
pub struct CsvPermissionRow {
    #[serde(default = "default_lib", alias = "library")]
    pub Library: String,
    #[serde(alias = "relative_folder_path", alias = "FolderPath", alias = "Folder")]
    pub RelativeFolderPath: String,
    #[serde(default, alias = "copy_parent_roles")]
    pub CopyParentRoles: String,
    #[serde(default, alias = "clear_subscopes")]
    pub ClearSubscopes: String,
    #[serde(alias = "Group", alias = "principal", alias = "group")]
    pub Principal: String,
    #[serde(default, alias = "PrincipalName", alias = "principal_display_name")]
    pub PrincipalDisplayName: Option<String>,
    #[serde(default = "default_principal_type", alias = "principal_type")]
    pub PrincipalType: String,
    #[serde(alias = "role", alias = "Permission")]
    pub Role: String,
    #[serde(default = "default_true_str", alias = "create_group_if_missing")]
    pub CreateGroupIfMissing: String,
}

fn default_lib() -> String {
    "Documents".to_string()
}

fn default_principal_type() -> String {
    "SharePointGroup".to_string()
}

fn default_true_str() -> String {
    "true".to_string()
}

fn parse_bool_loose(s: &str, default_val: bool) -> bool {
    match s.trim().to_lowercase().as_str() {
        "true" | "1" | "yes" | "si" | "sí" => true,
        "false" | "0" | "no" => false,
        "" => default_val,
        _ => default_val,
    }
}

pub struct MatrixImporter;

impl MatrixImporter {
    /// Load a permission matrix from a `.csv` or `.json` file and sort folder specs top-down by folder depth.
    pub fn load_from_file(
        path: &Path,
        default_library_override: Option<&str>,
    ) -> Result<Vec<MatrixFolderSpec>> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("No se pudo leer el archivo '{}'", path.display()))?;
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        if ext == "json" || content.trim_start().starts_with('[') {
            Self::parse_json(&content, default_library_override)
        } else {
            Self::parse_csv(&content, default_library_override)
        }
    }

    pub fn parse_json(
        content: &str,
        default_library_override: Option<&str>,
    ) -> Result<Vec<MatrixFolderSpec>> {
        let mut specs: Vec<MatrixFolderSpec> =
            serde_json::from_str(content).context("Error parseando matriz JSON")?;

        if let Some(lib_ov) = default_library_override.filter(|s| !s.trim().is_empty()) {
            for s in &mut specs {
                if s.library.is_empty() || s.library.eq_ignore_ascii_case("Documents") {
                    s.library = lib_ov.to_string();
                }
            }
        }

        Self::sort_specs_top_down(&mut specs);
        Ok(specs)
    }

    pub fn parse_csv(
        content: &str,
        default_library_override: Option<&str>,
    ) -> Result<Vec<MatrixFolderSpec>> {
        let mut rdr = csv::ReaderBuilder::new()
            .trim(csv::Trim::All)
            .flexible(true)
            .from_reader(content.as_bytes());

        let mut grouped: BTreeMap<(String, String), MatrixFolderSpec> = BTreeMap::new();

        for result in rdr.deserialize() {
            let row: CsvPermissionRow =
                result.context("Error leyendo fila del archivo CSV de permisos")?;
            let rel_path = row.RelativeFolderPath.trim_matches('/').to_string();
            if rel_path.is_empty() {
                continue;
            }
            let lib = match default_library_override.filter(|s| !s.trim().is_empty()) {
                Some(ov) if row.Library.is_empty() || row.Library.eq_ignore_ascii_case("Documents") => {
                    ov.to_string()
                }
                _ if row.Library.trim().is_empty() => "Documents".to_string(),
                _ => row.Library.trim().to_string(),
            };

            let copy_parent = parse_bool_loose(&row.CopyParentRoles, false);
            let clear_subs = parse_bool_loose(&row.ClearSubscopes, false);
            let create_grp = parse_bool_loose(&row.CreateGroupIfMissing, true);

            let entry = grouped
                .entry((lib.clone(), rel_path.clone()))
                .or_insert_with(|| MatrixFolderSpec {
                    library: lib,
                    relative_folder_path: rel_path,
                    copy_parent_roles: copy_parent,
                    clear_subscopes: clear_subs,
                    permissions: Vec::new(),
                });

            if !row.Principal.trim().is_empty() && !row.Role.trim().is_empty() {
                entry.permissions.push(MatrixPermissionGrant {
                    principal: row.Principal.trim().to_string(),
                    principal_name: row.PrincipalDisplayName,
                    principal_type: if row.PrincipalType.trim().is_empty() {
                        "SharePointGroup".to_string()
                    } else {
                        row.PrincipalType.trim().to_string()
                    },
                    role: row.Role.trim().to_string(),
                    create_group_if_missing: create_grp,
                });
            }
        }

        let mut specs: Vec<MatrixFolderSpec> = grouped.into_values().collect();
        if specs.is_empty() {
            return Err(anyhow!(
                "El archivo CSV no contiene filas válidas con RelativeFolderPath"
            ));
        }
        Self::sort_specs_top_down(&mut specs);
        Ok(specs)
    }

    /// Sort folder specifications top-down by folder path depth so parent folders are always created and configured before child folders.
    pub fn sort_specs_top_down(specs: &mut [MatrixFolderSpec]) {
        specs.sort_by(|a, b| {
            let depth_a = a
                .relative_folder_path
                .trim_matches('/')
                .split('/')
                .filter(|s| !s.is_empty())
                .count();
            let depth_b = b
                .relative_folder_path
                .trim_matches('/')
                .split('/')
                .filter(|s| !s.is_empty())
                .count();
            a.library
                .to_lowercase()
                .cmp(&b.library.to_lowercase())
                .then_with(|| depth_a.cmp(&depth_b))
                .then_with(|| {
                    a.relative_folder_path
                        .to_lowercase()
                        .cmp(&b.relative_folder_path.to_lowercase())
                })
        });
    }

    /// Convert a slice of `MatrixFolderSpec` into atomic `StagingOp` items inside a `StagingQueue`.
    pub fn specs_to_staging_queue(site_url: &str, specs: &[MatrixFolderSpec]) -> StagingQueue {
        let mut queue = StagingQueue::new();
        let mut ensured_groups: HashSet<String> = HashSet::new();

        // 1. Pre-create any SharePoint groups referenced with `create_group_if_missing = true`
        for spec in specs {
            for perm in &spec.permissions {
                if perm.principal_type.eq_ignore_ascii_case("SharePointGroup")
                    && perm.create_group_if_missing
                {
                    let key = perm.principal.to_lowercase();
                    if ensured_groups.insert(key) {
                        queue.push(StagingOp::CreateGroup {
                            site_url: site_url.to_string(),
                            group_title: perm.principal.clone(),
                            description: format!(
                                "Grupo aprovisionado desde matriz para {}",
                                spec.relative_folder_path
                            ),
                        });
                    }
                }
            }
        }

        // 2. For each folder (already sorted top-down by depth): CreateFolder -> BreakInheritance -> AssignFolderRole
        for spec in specs {
            let clean_path = spec.relative_folder_path.trim_matches('/').to_string();
            if clean_path.is_empty() {
                continue;
            }
            queue.push(StagingOp::CreateFolder {
                site_url: site_url.to_string(),
                library_title: spec.library.clone(),
                relative_folder_path: clean_path.clone(),
            });

            if !spec.permissions.is_empty() {
                queue.push(StagingOp::BreakFolderInheritance {
                    site_url: site_url.to_string(),
                    library_title: spec.library.clone(),
                    relative_folder_path: clean_path.clone(),
                    copy_parent_roles: spec.copy_parent_roles,
                    clear_subscopes: spec.clear_subscopes,
                });

                for perm in &spec.permissions {
                    queue.push(StagingOp::AssignFolderRole {
                        site_url: site_url.to_string(),
                        library_title: spec.library.clone(),
                        relative_folder_path: clean_path.clone(),
                        principal_name: perm.principal.clone(),
                        principal_type: perm.principal_type.clone(),
                        role_name: perm.role.clone(),
                        create_group_if_missing: perm.create_group_if_missing,
                    });
                }
            }
        }

        queue
    }

    /// Export unique folder permissions from a SharePoint library to a `.json` or `.csv` file.
    pub async fn export_library_permissions_to_file(
        spo: &SpoClient,
        site_url: &str,
        library_name: &str,
        output_path: &Path,
    ) -> Result<usize> {
        let lib = spo.resolve_library(site_url, library_name).await?;
        let unique_folders = spo
            .get_folders_with_unique_permissions(site_url, &lib.title, &lib.server_relative_url)
            .await?;

        let mut specs = Vec::new();
        for folder in unique_folders {
            let mut perms = Vec::new();
            for ra in folder.role_assignments {
                let clean_principal = if ra.principal_type == "SharePointGroup" {
                    ra.principal_title.clone()
                } else {
                    ra.principal_login_name
                        .split('|')
                        .next_back()
                        .unwrap_or(&ra.principal_login_name)
                        .to_string()
                };
                for role in ra.roles {
                    perms.push(MatrixPermissionGrant {
                        principal: clean_principal.clone(),
                        principal_name: Some(ra.principal_title.clone()),
                        principal_type: ra.principal_type.clone(),
                        role,
                        create_group_if_missing: true,
                    });
                }
            }
            specs.push(MatrixFolderSpec {
                library: lib.title.clone(),
                relative_folder_path: folder.relative_path,
                copy_parent_roles: false,
                clear_subscopes: false,
                permissions: perms,
            });
        }

        let ext = output_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("json")
            .to_lowercase();

        if ext == "csv" {
            let mut wtr = csv::Writer::from_path(output_path)?;
            for spec in &specs {
                for p in &spec.permissions {
                    wtr.serialize(CsvPermissionRow {
                        Library: spec.library.clone(),
                        RelativeFolderPath: spec.relative_folder_path.clone(),
                        CopyParentRoles: spec.copy_parent_roles.to_string(),
                        ClearSubscopes: spec.clear_subscopes.to_string(),
                        Principal: p.principal.clone(),
                        PrincipalDisplayName: p.principal_name.clone(),
                        PrincipalType: p.principal_type.clone(),
                        Role: p.role.clone(),
                        CreateGroupIfMissing: p.create_group_if_missing.to_string(),
                    })?;
                }
            }
            wtr.flush()?;
        } else {
            let json = serde_json::to_string_pretty(&specs)?;
            fs::write(output_path, json)?;
        }

        Ok(specs.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_csv_parse_and_top_down_sort() {
        let csv_data = "\
Library,RelativeFolderPath,CopyParentRoles,ClearSubscopes,Group,Role,CreateGroupIfMissing
Documentos,01_Management/SubA/Deep,false,false,SPO_Deep,Full Control,true
Documentos,01_Management,false,false,SPO_Mgmt_Read,Read,true
Documentos,01_Management,false,false,SPO_Mgmt_Edit,Edit,true
Documentos,01_Management/SubA,false,false,SPO_SubA,Contribute,true
";
        let specs = MatrixImporter::parse_csv(csv_data, None).unwrap();
        assert_eq!(specs.len(), 3);
        assert_eq!(specs[0].relative_folder_path, "01_Management");
        assert_eq!(specs[0].permissions.len(), 2);
        assert_eq!(specs[1].relative_folder_path, "01_Management/SubA");
        assert_eq!(specs[2].relative_folder_path, "01_Management/SubA/Deep");

        let queue = MatrixImporter::specs_to_staging_queue(
            "https://acciona365.sharepoint.com/sites/Test",
            &specs,
        );
        // 4 unique groups + 3 folders + 3 break inheritance + 4 role assignments = 14 ops
        assert_eq!(queue.len(), 14);
    }

    #[test]
    fn test_json_matrix_parse() {
        let json_data = r#"[
          {
            "RelativeFolderPath": "02_Child/Sub",
            "CopyParentRoles": false,
            "ClearSubscopes": false,
            "Permissions": [
              { "Principal": "Grp_Child", "PrincipalType": "SharePointGroup", "Role": "Edit" }
            ]
          },
          {
            "RelativeFolderPath": "02_Child",
            "CopyParentRoles": false,
            "ClearSubscopes": false,
            "Permissions": [
              { "Principal": "Grp_Root", "PrincipalType": "SharePointGroup", "Role": "Read" }
            ]
          }
        ]"#;
        let specs = MatrixImporter::parse_json(json_data, Some("Documentos")).unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].relative_folder_path, "02_Child");
        assert_eq!(specs[0].library, "Documentos");
        assert_eq!(specs[1].relative_folder_path, "02_Child/Sub");
    }
}
