use crate::api::spo::SpoClient;
use crate::api::{check_json_response, send_with_retry};
use crate::models::{FolderPermissionEntry, SpoFolder, SpoLibrary, SpoRoleDefinition};
use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::json;

const SPO_JSON_ACCEPT: &str = "application/json;odata=nometadata";
const SPO_JSON_CONTENT: &str = "application/json;odata=nometadata";

const SYSTEM_LIBRARY_TITLES: &[&str] = &[
    "Style Library",
    "Site Assets",
    "Activos del sitio",
    "Site Pages",
    "Páginas del sitio",
    "Form Templates",
    "Plantillas de formulario",
    "Preservation Hold Library",
    "Site Collection Documents",
    "Site Collection Images",
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
    #[serde(rename = "RootFolder")]
    root_folder: Option<RawRootFolder>,
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
    /// List non-system Document Libraries (`BaseTemplate == 101`) in a site.
    pub async fn get_document_libraries(&self, site_url: &str) -> Result<Vec<SpoLibrary>> {
        let base = site_url.trim_end_matches('/');
        let token = self.token(base).await?;
        let url = format!(
            "{}/_api/web/lists?$select=Id,Title,BaseTemplate,Hidden,ItemCount,RootFolder/ServerRelativeUrl&$expand=RootFolder&$filter=BaseTemplate eq 101 and Hidden eq false",
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
            .filter(|l| {
                l.base_template == 101
                    && !l.hidden
                    && !SYSTEM_LIBRARY_TITLES
                        .iter()
                        .any(|sys| sys.eq_ignore_ascii_case(&l.title))
            })
            .filter_map(|l| {
                let srv_url = l.root_folder?.server_relative_url;
                if srv_url.ends_with("/_catalogs") || srv_url.contains("/_catalogs/") {
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
}
