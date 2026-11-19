use serde::{Deserialize, Serialize};

/// Role of the current user in a SharePoint Online site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SiteRole {
    SiteAdmin,
    Owner,
    M365GroupOwner,
    AdminAndOwner,
}

impl SiteRole {
    pub fn badge(&self) -> &'static str {
        match self {
            SiteRole::SiteAdmin => "[Admin]",
            SiteRole::Owner => "[Owner]",
            SiteRole::M365GroupOwner => "[M365 Owner]",
            SiteRole::AdminAndOwner => "[Admin+Owner]",
        }
    }

    pub fn matches_filter(&self, filter: &str) -> bool {
        match filter.to_lowercase().as_str() {
            "admin" => matches!(self, SiteRole::SiteAdmin | SiteRole::AdminAndOwner),
            "owner" => matches!(
                self,
                SiteRole::Owner | SiteRole::M365GroupOwner | SiteRole::AdminAndOwner
            ),
            _ => true,
        }
    }
}

/// Represents a SharePoint Online site where the user is Administrator or Owner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpoSite {
    pub title: String,
    pub url: String,
    pub description: Option<String>,
    pub role: SiteRole,
    pub is_site_admin: bool,
}

/// Classification of a SharePoint site group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroupKind {
    AssociatedOwner,
    AssociatedMember,
    AssociatedVisitor,
    Custom,
}

impl GroupKind {
    pub fn badge(&self) -> &'static str {
        match self {
            GroupKind::AssociatedOwner => "[Owner]",
            GroupKind::AssociatedMember => "[Member]",
            GroupKind::AssociatedVisitor => "[Visitor]",
            GroupKind::Custom => "[Custom]",
        }
    }

    pub fn sort_rank(&self) -> u8 {
        match self {
            GroupKind::AssociatedOwner => 0,
            GroupKind::AssociatedMember => 1,
            GroupKind::AssociatedVisitor => 2,
            GroupKind::Custom => 3,
        }
    }
}

/// SharePoint site group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpoGroup {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub owner_title: String,
    pub kind: GroupKind,
    pub permission_levels: Vec<String>,
}

/// Member of a SharePoint group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpoUser {
    pub id: i64,
    pub title: String,
    pub email: String,
    pub login_name: String,
    pub upn: Option<String>,
    pub is_site_admin: bool,
    pub principal_type: i32, // 1 = User, 4 = SecurityGroup, 8 = SharePointGroup
}

impl SpoUser {
    pub fn clean_identifier(&self) -> String {
        if !self.email.is_empty() {
            self.email.clone()
        } else if let Some(ref upn) = self.upn {
            if !upn.is_empty() {
                return upn.clone();
            }
            self.extract_from_login()
        } else {
            self.extract_from_login()
        }
    }

    fn extract_from_login(&self) -> String {
        self.login_name
            .split('|')
            .next_back()
            .unwrap_or(&self.login_name)
            .to_string()
    }
}

/// User returned from Microsoft Graph or SharePoint People Picker directory search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryUser {
    pub display_name: String,
    pub email: String,
    pub upn: String,
    pub login_name: String,
    pub job_title: Option<String>,
    pub department: Option<String>,
}

impl DirectoryUser {
    pub fn identifier(&self) -> &str {
        if !self.upn.is_empty() {
            &self.upn
        } else if !self.email.is_empty() {
            &self.email
        } else {
            &self.login_name
        }
    }
}

/// SharePoint Document Library (BaseTemplate = 101).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpoLibrary {
    pub id: String,
    pub title: String,
    pub server_relative_url: String,
    pub item_count: i64,
    pub is_default_documents: bool,
    #[serde(default)]
    pub exclude_from_offline_client: bool,
}

/// Offline client synchronization status (`ExcludeFromOfflineClient`) for a site and its libraries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteSyncStatus {
    pub site_url: String,
    pub site_title: String,
    pub exclude_from_offline_client: bool,
    pub sync_enabled: bool,
    pub libraries: Vec<LibrarySyncStatus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subsites: Vec<SiteSyncStatus>,
}

/// Offline client synchronization status for a single SharePoint Document Library.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibrarySyncStatus {
    pub id: String,
    pub title: String,
    pub server_relative_url: String,
    pub is_default_documents: bool,
    pub exclude_from_offline_client: bool,
    pub sync_enabled: bool,
}

/// Folder inside a SharePoint Document Library.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpoFolder {
    pub name: String,
    pub server_relative_url: String,
    pub relative_path: String,
    pub item_id: Option<i64>,
    pub has_unique_role_assignments: bool,
    pub role_assignments: Vec<FolderPermissionEntry>,
}

/// Permission entry assigned to a folder or list item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderPermissionEntry {
    pub principal_id: i64,
    pub principal_title: String,
    pub principal_login_name: String,
    pub principal_type: String, // "User", "SecurityGroup", "SharePointGroup"
    pub roles: Vec<String>,
}

/// SharePoint Role Definition (Permission Level, e.g. Full Control, Edit, Contribute, Read).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpoRoleDefinition {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub role_type_kind: i32, // 2=Reader, 3=Contributor, 4=WebDesigner(Edit), 5=Administrator(Full Control)
}

/// Atomic staged operation waiting to be applied or previewed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op_type", content = "params")]
pub enum StagingOp {
    // Phase 1: Group membership
    AddUserToGroup {
        site_url: String,
        group_id: i64,
        group_title: String,
        user_identifier: String,
        user_display_name: String,
    },
    RemoveUserFromGroup {
        site_url: String,
        group_id: i64,
        group_title: String,
        user_id: Option<i64>,
        user_identifier: String,
        user_display_name: String,
    },
    // Phase 2: Libraries, Folders, Groups & Folder-level permissions
    CreateLibrary {
        site_url: String,
        library_title: String,
        description: String,
    },
    CreateGroup {
        site_url: String,
        group_title: String,
        description: String,
    },
    CreateFolder {
        site_url: String,
        library_title: String,
        relative_folder_path: String,
    },
    BreakFolderInheritance {
        site_url: String,
        library_title: String,
        relative_folder_path: String,
        copy_parent_roles: bool,
        clear_subscopes: bool,
    },
    AssignFolderRole {
        site_url: String,
        library_title: String,
        relative_folder_path: String,
        principal_name: String,
        principal_type: String, // "SharePointGroup", "User", "SecurityGroup"
        role_name: String,
        create_group_if_missing: bool,
    },
    // Phase 3: Site-level role assignment & Server-side copy
    AssignSiteRole {
        site_url: String,
        group_title: String,
        role_name: String,
    },
    ServerSideCopy {
        source_site_url: String,
        target_site_url: String,
        source_server_relative_url: String,
        target_folder_server_relative_url: String,
        is_folder: bool,
        overwrite: bool,
    },
    // Phase 4: Offline Sync / OneDrive Shortcut control (`ExcludeFromOfflineClient`)
    SetSiteOfflineSync {
        site_url: String,
        site_title: String,
        exclude_from_offline_client: bool,
    },
    SetLibraryOfflineSync {
        site_url: String,
        library_id: String,
        library_title: String,
        exclude_from_offline_client: bool,
    },
}

impl StagingOp {
    pub fn op_type_code(&self) -> &'static str {
        match self {
            StagingOp::AddUserToGroup { .. } => "ADD_USER_TO_GROUP",
            StagingOp::RemoveUserFromGroup { .. } => "REMOVE_USER_FROM_GROUP",
            StagingOp::CreateLibrary { .. } => "CREATE_LIBRARY",
            StagingOp::CreateGroup { .. } => "CREATE_GROUP",
            StagingOp::CreateFolder { .. } => "CREATE_FOLDER",
            StagingOp::BreakFolderInheritance { .. } => "BREAK_INHERITANCE",
            StagingOp::AssignFolderRole { .. } => "ASSIGN_FOLDER_ROLE",
            StagingOp::AssignSiteRole { .. } => "ASSIGN_SITE_ROLE",
            StagingOp::ServerSideCopy { .. } => "COPY_SERVER_SIDE_ITEM",
            StagingOp::SetSiteOfflineSync { .. } => "SET_SITE_OFFLINE_SYNC",
            StagingOp::SetLibraryOfflineSync { .. } => "SET_LIBRARY_OFFLINE_SYNC",
        }
    }

    pub fn site_url(&self) -> &str {
        match self {
            StagingOp::AddUserToGroup { site_url, .. }
            | StagingOp::RemoveUserFromGroup { site_url, .. }
            | StagingOp::CreateLibrary { site_url, .. }
            | StagingOp::CreateGroup { site_url, .. }
            | StagingOp::CreateFolder { site_url, .. }
            | StagingOp::BreakFolderInheritance { site_url, .. }
            | StagingOp::AssignFolderRole { site_url, .. }
            | StagingOp::AssignSiteRole { site_url, .. }
            | StagingOp::SetSiteOfflineSync { site_url, .. }
            | StagingOp::SetLibraryOfflineSync { site_url, .. } => site_url,
            StagingOp::ServerSideCopy {
                target_site_url, ..
            } => target_site_url,
        }
    }

    pub fn label(&self) -> String {
        match self {
            StagingOp::AddUserToGroup {
                group_title,
                user_display_name,
                user_identifier,
                ..
            } => format!(
                "[+] Añadir '{}' ({}) al grupo '{}'",
                user_display_name, user_identifier, group_title
            ),
            StagingOp::RemoveUserFromGroup {
                group_title,
                user_display_name,
                user_identifier,
                ..
            } => format!(
                "[-] Remover '{}' ({}) del grupo '{}'",
                user_display_name, user_identifier, group_title
            ),
            StagingOp::CreateLibrary { library_title, .. } => {
                format!("[+Bib] Crear biblioteca '{}'", library_title)
            }
            StagingOp::CreateGroup { group_title, .. } => {
                format!("[+Grp] Crear grupo SharePoint '{}'", group_title)
            }
            StagingOp::CreateFolder {
                library_title,
                relative_folder_path,
                ..
            } => format!(
                "[+Dir] Crear carpeta '{}/{}'",
                library_title, relative_folder_path
            ),
            StagingOp::BreakFolderInheritance {
                library_title,
                relative_folder_path,
                copy_parent_roles,
                clear_subscopes,
                ..
            } => format!(
                "[Perm] Romper herencia en '{}/{}' (copy={}, clear={})",
                library_title, relative_folder_path, copy_parent_roles, clear_subscopes
            ),
            StagingOp::AssignFolderRole {
                library_title,
                relative_folder_path,
                principal_name,
                role_name,
                ..
            } => format!(
                "[+Rol] Asignar '{}' a '{}' en '{}/{}'",
                role_name, principal_name, library_title, relative_folder_path
            ),
            StagingOp::AssignSiteRole {
                group_title,
                role_name,
                ..
            } => format!(
                "[+SitioRol] Asignar '{}' al grupo '{}' en el sitio",
                role_name, group_title
            ),
            StagingOp::ServerSideCopy {
                source_server_relative_url,
                target_folder_server_relative_url,
                is_folder,
                ..
            } => {
                let kind = if *is_folder { "Carpeta" } else { "Archivo" };
                format!(
                    "[Copy {}] '{}' -> '{}'",
                    kind, source_server_relative_url, target_folder_server_relative_url
                )
            }
            StagingOp::SetSiteOfflineSync {
                site_title,
                exclude_from_offline_client,
                ..
            } => {
                if *exclude_from_offline_client {
                    format!(
                        "[-Sync] Desactivar sincronización offline en el sitio '{}'",
                        site_title
                    )
                } else {
                    format!(
                        "[+Sync] Activar sincronización offline en el sitio '{}'",
                        site_title
                    )
                }
            }
            StagingOp::SetLibraryOfflineSync {
                library_title,
                exclude_from_offline_client,
                ..
            } => {
                if *exclude_from_offline_client {
                    format!(
                        "[-Sync] Desactivar sincronización offline en biblioteca '{}'",
                        library_title
                    )
                } else {
                    format!(
                        "[+Sync] Activar sincronización offline en biblioteca '{}'",
                        library_title
                    )
                }
            }
        }
    }
}

/// Payload stored in `reverse_payload` column of `transaction_operations` for deterministic LIFO Rollback.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "reverse_type")]
pub enum ReversePayload {
    RemoveAddedUser {
        site_url: String,
        group_id: i64,
        user_id: Option<i64>,
        login_name: String,
    },
    RestoreRemovedUser {
        site_url: String,
        group_id: i64,
        login_name: String,
    },
    DeleteCreatedGroup {
        site_url: String,
        group_id: i64,
    },
    DeleteCreatedLibrary {
        site_url: String,
        library_id: String,
    },
    DeleteCreatedFolder {
        site_url: String,
        folder_server_relative_url: String,
    },
    ResetFolderInheritance {
        site_url: String,
        folder_server_relative_url: String,
        had_unique_roles_before: bool,
    },
    RemoveFolderRoleAssignment {
        site_url: String,
        folder_server_relative_url: String,
        principal_id: i64,
        role_def_id: i64,
    },
    RemoveSiteRoleAssignment {
        site_url: String,
        principal_id: i64,
        role_def_id: i64,
    },
    RestoreSiteOfflineSync {
        site_url: String,
        previous_exclude_from_offline_client: bool,
    },
    RestoreLibraryOfflineSync {
        site_url: String,
        library_id: String,
        previous_exclude_from_offline_client: bool,
    },
    NoOp {
        reason: String,
    },
}

/// Transaction header stored in SQLite (`transactions` table).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRecord {
    pub id: i64,
    pub created_at: String,
    pub actor_upn: String,
    pub site_url: String,
    pub site_title: String,
    pub source: String,
    pub note: Option<String>,
    pub status: String, // "APPLIED", "PARTIAL", "REVERTED", "FAILED"
    pub reverted_at: Option<String>,
    pub summary: String,
}

/// Individual atomic operation stored in SQLite (`transaction_operations` table).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionOpRecord {
    pub id: i64,
    pub transaction_id: i64,
    pub seq_order: i32,
    pub op_type: String,
    pub target_label: String,
    pub forward_payload: String,
    pub reverse_payload: String,
    pub status: String, // "APPLIED", "REVERTED", "FAILED"
    pub error_message: Option<String>,
}

/// Declarative entry for external CSV / JSON permission matrix (Phase 2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixFolderSpec {
    #[serde(default = "default_library_name", alias = "Library")]
    pub library: String,
    #[serde(alias = "RelativeFolderPath")]
    pub relative_folder_path: String,
    #[serde(default, alias = "CopyParentRoles")]
    pub copy_parent_roles: bool,
    #[serde(default, alias = "ClearSubscopes")]
    pub clear_subscopes: bool,
    #[serde(default, alias = "Permissions")]
    pub permissions: Vec<MatrixPermissionGrant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixPermissionGrant {
    #[serde(alias = "Principal", alias = "Group")]
    pub principal: String,
    #[serde(default, alias = "PrincipalName", alias = "PrincipalDisplayName")]
    pub principal_name: Option<String>,
    #[serde(default = "default_principal_type", alias = "PrincipalType")]
    pub principal_type: String,
    #[serde(alias = "Role")]
    pub role: String,
    #[serde(default = "default_true", alias = "CreateGroupIfMissing")]
    pub create_group_if_missing: bool,
}

fn default_library_name() -> String {
    "Documents".to_string()
}

fn default_principal_type() -> String {
    "SharePointGroup".to_string()
}

fn default_true() -> bool {
    true
}

/// Configuration for Phase 3 full site cloning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneConfig {
    pub source_site_url: String,
    pub target_site_url: String,
    pub libraries_filter: Option<Vec<String>>,
    pub target_base_folder: String,
    pub include_groups: bool,
    pub include_files: bool,
    pub include_unique_permissions: bool,
    pub overwrite: bool,
    pub note: Option<String>,
}
