use crate::api::spo::SpoClient;
use crate::api::{check_json_response, send_with_retry};
use crate::models::{FolderPermissionEntry, SpoFolder};
use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::json;
use url::Url;

const SPO_JSON_ACCEPT: &str = "application/json;odata=nometadata";
const SPO_JSON_CONTENT: &str = "application/json;odata=nometadata";

#[derive(Debug, Clone)]
pub struct LibraryRootItem {
    pub name: String,
    pub server_relative_url: String,
    pub is_folder: bool,
}

#[derive(Debug, Deserialize)]
struct SpoValueList<T> {
    value: Vec<T>,
    #[serde(rename = "odata.nextLink", alias = "@odata.nextLink")]
    next_link: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawSimpleItem {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "ServerRelativeUrl")]
    server_relative_url: String,
}

#[derive(Debug, Deserialize)]
struct RawListItemUniqueCheck {
    #[serde(rename = "Id")]
    id: i64,
    #[serde(rename = "FileSystemObjectType", default)]
    fs_obj_type: i32,
    #[serde(rename = "HasUniqueRoleAssignments", default)]
    has_unique_role_assignments: bool,
    #[serde(rename = "FileRef")]
    file_ref: Option<String>,
    #[serde(rename = "FileLeafRef")]
    file_leaf_ref: Option<String>,
}

impl SpoClient {
    /// List top-level folders and files inside a Document Library root folder (excluding `Forms`).
    pub async fn list_library_root_items(
        &self,
        site_url: &str,
        library_server_relative_url: &str,
    ) -> Result<Vec<LibraryRootItem>> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let escaped = library_server_relative_url
            .trim_end_matches('/')
            .replace('\'', "''");

        let folders_url = format!(
            "{}/_api/web/GetFolderByServerRelativePath(decodedurl='{}')/Folders?$select=Name,ServerRelativeUrl",
            base,
            urlencoding::encode(&escaped)
        );
        let files_url = format!(
            "{}/_api/web/GetFolderByServerRelativePath(decodedurl='{}')/Files?$select=Name,ServerRelativeUrl",
            base,
            urlencoding::encode(&escaped)
        );

        let (folders_resp, files_resp) = tokio::try_join!(
            send_with_retry(|| {
                self.http
                    .get(&folders_url)
                    .bearer_auth(&token)
                    .header("Accept", SPO_JSON_ACCEPT)
            }),
            send_with_retry(|| {
                self.http
                    .get(&files_url)
                    .bearer_auth(&token)
                    .header("Accept", SPO_JSON_ACCEPT)
            })
        )?;

        let raw_folders: SpoValueList<RawSimpleItem> =
            check_json_response(folders_resp, "Error listando carpetas raíz de biblioteca")
                .await?;
        let raw_files: SpoValueList<RawSimpleItem> =
            check_json_response(files_resp, "Error listando archivos raíz de biblioteca").await?;

        let mut items = Vec::new();
        for f in raw_folders.value {
            if f.name.eq_ignore_ascii_case("Forms") || f.name.starts_with('_') {
                continue;
            }
            items.push(LibraryRootItem {
                name: f.name,
                server_relative_url: f.server_relative_url,
                is_folder: true,
            });
        }
        for file in raw_files.value {
            items.push(LibraryRootItem {
                name: file.name,
                server_relative_url: file.server_relative_url,
                is_folder: false,
            });
        }

        items.sort_by(|a, b| {
            b.is_folder
                .cmp(&a.is_folder)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(items)
    }

    /// Perform a Server-Side Copy of a Folder or File inside SharePoint Online (`SP.MoveCopyUtil`)
    /// preserving version history (`ResetAuthorAndCreatedOnCopy: false`).
    pub async fn server_side_copy_item(
        &self,
        source_site_url: &str,
        source_server_relative_url: &str,
        target_site_url: &str,
        target_parent_folder_server_relative_url: &str,
        is_folder: bool,
        overwrite: bool,
    ) -> Result<()> {
        let src_base = source_site_url.trim_end_matches('/');
        let token = self.token(src_base).await?;

        let src_origin = Self::extract_origin(source_site_url)?;
        let tgt_origin = Self::extract_origin(target_site_url)?;

        let item_name = source_server_relative_url
            .trim_end_matches('/')
            .split('/')
            .next_back()
            .unwrap_or("item");

        let src_abs = format!(
            "{}/{}",
            src_origin.trim_end_matches('/'),
            source_server_relative_url.trim_start_matches('/')
        );
        let dst_abs = format!(
            "{}/{}/{}",
            tgt_origin.trim_end_matches('/'),
            target_parent_folder_server_relative_url.trim_matches('/'),
            item_name
        );

        let endpoint = if is_folder {
            format!("{}/_api/SP.MoveCopyUtil.CopyFolderByPath()", src_base)
        } else {
            format!("{}/_api/SP.MoveCopyUtil.CopyFileByPath()", src_base)
        };

        let body = json!({
            "srcPath": { "DecodedUrl": src_abs },
            "destPath": { "DecodedUrl": dst_abs },
            "overwrite": overwrite,
            "options": {
                "KeepBoth": false,
                "ResetAuthorAndCreatedOnCopy": false,
                "ShouldBypassSharedLocks": true
            }
        });

        let resp = send_with_retry(|| {
            self.http
                .post(&endpoint)
                .bearer_auth(&token)
                .header("Accept", SPO_JSON_ACCEPT)
                .header("Content-Type", SPO_JSON_CONTENT)
                .json(&body)
        })
        .await?;

        if !resp.status().is_success() {
            let err_txt = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Error en Server-Side Copy de '{}' a '{}': {}",
                src_abs,
                dst_abs,
                err_txt
            ));
        }

        Ok(())
    }

    /// Scan all folders in a Document Library that have broken permission inheritance (`HasUniqueRoleAssignments == true`),
    /// sorted top-down by relative path depth so parent folders are processed before child folders.
    pub async fn get_folders_with_unique_permissions(
        &self,
        site_url: &str,
        library_title: &str,
        library_server_relative_url: &str,
    ) -> Result<Vec<SpoFolder>> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let escaped_title = library_title.replace('\'', "''");
        let lib_root = library_server_relative_url.trim_end_matches('/');

        let mut next_url = Some(format!(
            "{}/_api/web/lists/getbytitle('{}')/items?$select=Id,FileSystemObjectType,HasUniqueRoleAssignments,FileRef,FileLeafRef&$top=500",
            base,
            urlencoding::encode(&escaped_title)
        ));

        let mut unique_folders = Vec::new();

        while let Some(url) = next_url.take() {
            let resp = send_with_retry(|| {
                self.http
                    .get(&url)
                    .bearer_auth(&token)
                    .header("Accept", SPO_JSON_ACCEPT)
            })
            .await?;

            if !resp.status().is_success() {
                break;
            }

            let page: SpoValueList<RawListItemUniqueCheck> = resp.json().await?;
            next_url = page.next_link;

            for item in page.value {
                if !item.has_unique_role_assignments || item.fs_obj_type != 1 {
                    continue;
                }
                let file_ref = match item.file_ref {
                    Some(r) if !r.is_empty() => r,
                    _ => continue,
                };
                let rel_path = file_ref
                    .strip_prefix(lib_root)
                    .unwrap_or(&file_ref)
                    .trim_matches('/')
                    .to_string();

                if rel_path.is_empty()
                    || rel_path.eq_ignore_ascii_case("Forms")
                    || rel_path.starts_with("Forms/")
                {
                    continue;
                }

                let assignments: Vec<FolderPermissionEntry> = self
                    .get_folder_role_assignments(site_url, &file_ref)
                    .await
                    .unwrap_or_default();

                if !assignments.is_empty() {
                    unique_folders.push(SpoFolder {
                        name: item.file_leaf_ref.unwrap_or_else(|| rel_path.clone()),
                        server_relative_url: file_ref,
                        relative_path: rel_path,
                        item_id: Some(item.id),
                        item_count: 0,
                        has_unique_role_assignments: true,
                        role_assignments: assignments,
                    });
                }
            }
        }

        // Sort strictly top-down by folder depth so parent folders are configured before child folders
        unique_folders.sort_by(|a, b| {
            let depth_a = a.relative_path.split('/').count();
            let depth_b = b.relative_path.split('/').count();
            depth_a
                .cmp(&depth_b)
                .then_with(|| a.relative_path.to_lowercase().cmp(&b.relative_path.to_lowercase()))
        });

        Ok(unique_folders)
    }

    pub fn extract_origin(site_url: &str) -> Result<String> {
        let parsed = Url::parse(site_url)?;
        let host = parsed
            .host_str()
            .ok_or_else(|| anyhow!("URL sin host: {}", site_url))?;
        Ok(format!("{}://{}", parsed.scheme(), host))
    }
}
