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
    #[serde(default)]
    pub views_total: usize,
    #[serde(default)]
    pub views_hiding_shortcut: usize,
    #[serde(default)]
    pub views_hiding_sync: usize,
    #[serde(default)]
    pub shortcut_button_hidden: bool,
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
    #[serde(default)]
    pub views_total: usize,
    #[serde(default)]
    pub views_hiding_shortcut: usize,
    #[serde(default)]
    pub views_hiding_sync: usize,
    #[serde(default)]
    pub shortcut_button_hidden: bool,
}

/// Backup of a library view's `CustomFormatter` JSON before applying View Formatting changes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ViewFormatterBackup {
    pub view_id: String,
    pub view_title: String,
    pub previous_custom_formatter: String,
}

/// Folder inside a SharePoint Document Library.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpoFolder {
    pub name: String,
    pub server_relative_url: String,
    pub relative_path: String,
    pub item_id: Option<i64>,
    #[serde(default)]
    pub item_count: i64,
    pub has_unique_role_assignments: bool,
    pub role_assignments: Vec<FolderPermissionEntry>,
}

/// Permission entry assigned to a folder or list item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FolderPermissionEntry {
    pub principal_id: i64,
    pub principal_title: String,
    pub principal_login_name: String,
    pub principal_type: String, // "User", "SecurityGroup", "SharePointGroup"
    pub roles: Vec<String>,
}

/// Security scope information for a library root or ancestor folder along a path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FolderAncestorInfo {
    /// Empty string `""` for the library root, or relative path (e.g. `"01_Dir"` / `"01_Dir/Sub"`).
    pub relative_path: String,
    pub name: String,
    pub server_relative_url: String,
    pub has_unique_role_assignments: bool,
    pub role_assignments: Vec<FolderPermissionEntry>,
}

/// Resolved inheritance origin for a folder level.
#[derive(Debug, Clone)]
pub struct FolderInheritanceSummary {
    pub inherited_from_label: String,
    pub inherited_from_path: String,
    pub source_has_unique_roles: bool,
    pub inherited_roles: Vec<FolderPermissionEntry>,
}

impl FolderInheritanceSummary {
    /// Resolve the nearest ancestor with broken inheritance (`has_unique_role_assignments`),
    /// falling back to the library root (`ancestors.first()`).
    pub fn from_ancestors(ancestors: &[FolderAncestorInfo], lang: crate::i18n::Lang) -> Self {
        let source = ancestors
            .iter()
            .rev()
            .find(|a| a.has_unique_role_assignments)
            .or_else(|| ancestors.first());

        let Some(src) = source else {
            return Self {
                inherited_from_label: lang.tr("Biblioteca", "Library").to_string(),
                inherited_from_path: String::new(),
                source_has_unique_roles: false,
                inherited_roles: Vec::new(),
            };
        };

        let label = if src.relative_path.is_empty() {
            match lang {
                crate::i18n::Lang::Es => format!("Biblioteca '{}'", src.name),
                crate::i18n::Lang::En => format!("Library '{}'", src.name),
            }
        } else {
            src.relative_path.clone()
        };

        Self {
            inherited_from_label: label,
            inherited_from_path: src.relative_path.clone(),
            source_has_unique_roles: src.has_unique_role_assignments,
            inherited_roles: src.role_assignments.clone(),
        }
    }
}

/// Summary of how breaking/modifying permissions at a lower folder level impacts ancestor/child scopes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PermissionChangeImpact {
    pub conflicting_roles: Vec<String>,
    pub will_lose_parent_roles: bool,
    pub triggers_limited_access_upward: bool,
    pub wipes_child_unique_scopes: bool,
    pub disconnects_from_parent: bool,
}

/// Analyze how granting `new_role` to `principal_name` on a folder interacts with inherited/existing permissions.
pub fn analyze_permission_change_impact(
    target_has_unique_roles: bool,
    target_current_roles: &[FolderPermissionEntry],
    inheritance: &FolderInheritanceSummary,
    principal_name: &str,
    new_role: &str,
    copy_parent_roles: bool,
    clear_subscopes: bool,
) -> PermissionChangeImpact {
    let trimmed_principal = principal_name.trim();
    let effective_before: &[FolderPermissionEntry] = if target_has_unique_roles {
        target_current_roles
    } else {
        &inheritance.inherited_roles
    };

    let matches_principal = |entry: &FolderPermissionEntry| -> bool {
        !trimmed_principal.is_empty()
            && (entry.principal_title.eq_ignore_ascii_case(trimmed_principal)
                || entry.principal_login_name.eq_ignore_ascii_case(trimmed_principal))
    };

    let conflicting_roles = if target_has_unique_roles || copy_parent_roles {
        let mut roles = Vec::new();
        for entry in effective_before {
            if matches_principal(entry) {
                for r in &entry.roles {
                    if !r.eq_ignore_ascii_case(new_role)
                        && !roles.iter().any(|existing: &String| existing.eq_ignore_ascii_case(r))
                    {
                        roles.push(r.clone());
                    }
                }
            }
        }
        roles
    } else {
        Vec::new()
    };

    let will_lose_parent_roles =
        !target_has_unique_roles && !copy_parent_roles && !inheritance.inherited_roles.is_empty();

    let triggers_limited_access_upward =
        !trimmed_principal.is_empty() && !inheritance.inherited_roles.iter().any(matches_principal);

    PermissionChangeImpact {
        conflicting_roles,
        will_lose_parent_roles,
        triggers_limited_access_upward,
        wipes_child_unique_scopes: clear_subscopes,
        disconnects_from_parent: !target_has_unique_roles,
    }
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
    RemoveFolderRole {
        site_url: String,
        library_title: String,
        relative_folder_path: String,
        principal_name: String,
        principal_type: String,
        role_name: String,
    },
    ResetFolderInheritance {
        site_url: String,
        library_title: String,
        relative_folder_path: String,
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
        #[serde(default = "default_true")]
        apply_view_formatting: bool,
    },
    SetLibraryOneDriveShortcuts {
        site_url: String,
        library_id: String,
        library_title: String,
        hide_shortcut_button: bool,
    },
    // Destructive clone cleanup operations (non-reversible)
    DeleteLibrary {
        site_url: String,
        library_id: String,
        library_title: String,
    },
    DeleteFolder {
        site_url: String,
        folder_server_relative_url: String,
    },
    DeleteFile {
        site_url: String,
        file_server_relative_url: String,
    },
    DeleteGroup {
        site_url: String,
        group_id: i64,
        group_title: String,
    },
}

impl StagingOp {
    pub fn is_destructive(&self) -> bool {
        matches!(
            self,
            StagingOp::DeleteLibrary { .. }
                | StagingOp::DeleteFolder { .. }
                | StagingOp::DeleteFile { .. }
                | StagingOp::DeleteGroup { .. }
        )
    }

    pub fn op_type_code(&self) -> &'static str {
        match self {
            StagingOp::AddUserToGroup { .. } => "ADD_USER_TO_GROUP",
            StagingOp::RemoveUserFromGroup { .. } => "REMOVE_USER_FROM_GROUP",
            StagingOp::CreateLibrary { .. } => "CREATE_LIBRARY",
            StagingOp::CreateGroup { .. } => "CREATE_GROUP",
            StagingOp::CreateFolder { .. } => "CREATE_FOLDER",
            StagingOp::BreakFolderInheritance { .. } => "BREAK_INHERITANCE",
            StagingOp::AssignFolderRole { .. } => "ASSIGN_FOLDER_ROLE",
            StagingOp::RemoveFolderRole { .. } => "REMOVE_FOLDER_ROLE",
            StagingOp::ResetFolderInheritance { .. } => "RESET_INHERITANCE",
            StagingOp::AssignSiteRole { .. } => "ASSIGN_SITE_ROLE",
            StagingOp::ServerSideCopy { .. } => "COPY_SERVER_SIDE_ITEM",
            StagingOp::SetSiteOfflineSync { .. } => "SET_SITE_OFFLINE_SYNC",
            StagingOp::SetLibraryOfflineSync { .. } => "SET_LIBRARY_OFFLINE_SYNC",
            StagingOp::SetLibraryOneDriveShortcuts { .. } => "SET_LIBRARY_ONEDRIVE_SHORTCUTS",
            StagingOp::DeleteLibrary { .. } => "DELETE_LIBRARY",
            StagingOp::DeleteFolder { .. } => "DELETE_FOLDER",
            StagingOp::DeleteFile { .. } => "DELETE_FILE",
            StagingOp::DeleteGroup { .. } => "DELETE_GROUP",
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
            | StagingOp::RemoveFolderRole { site_url, .. }
            | StagingOp::ResetFolderInheritance { site_url, .. }
            | StagingOp::AssignSiteRole { site_url, .. }
            | StagingOp::SetSiteOfflineSync { site_url, .. }
            | StagingOp::SetLibraryOfflineSync { site_url, .. }
            | StagingOp::SetLibraryOneDriveShortcuts { site_url, .. }
            | StagingOp::DeleteLibrary { site_url, .. }
            | StagingOp::DeleteFolder { site_url, .. }
            | StagingOp::DeleteFile { site_url, .. }
            | StagingOp::DeleteGroup { site_url, .. } => site_url,
            StagingOp::ServerSideCopy {
                target_site_url, ..
            } => target_site_url,
        }
    }

    pub fn label(&self) -> String {
        self.label_for(crate::i18n::Lang::Es)
    }

    pub fn label_for(&self, lang: crate::i18n::Lang) -> String {
        use crate::i18n::Lang;
        match (self, lang) {
            (
                StagingOp::AddUserToGroup {
                    group_title,
                    user_display_name,
                    user_identifier,
                    ..
                },
                Lang::Es,
            ) => format!(
                "[+] Añadir '{}' ({}) al grupo '{}'",
                user_display_name, user_identifier, group_title
            ),
            (
                StagingOp::AddUserToGroup {
                    group_title,
                    user_display_name,
                    user_identifier,
                    ..
                },
                Lang::En,
            ) => format!(
                "[+] Add '{}' ({}) to group '{}'",
                user_display_name, user_identifier, group_title
            ),
            (
                StagingOp::RemoveUserFromGroup {
                    group_title,
                    user_display_name,
                    user_identifier,
                    ..
                },
                Lang::Es,
            ) => format!(
                "[-] Remover '{}' ({}) del grupo '{}'",
                user_display_name, user_identifier, group_title
            ),
            (
                StagingOp::RemoveUserFromGroup {
                    group_title,
                    user_display_name,
                    user_identifier,
                    ..
                },
                Lang::En,
            ) => format!(
                "[-] Remove '{}' ({}) from group '{}'",
                user_display_name, user_identifier, group_title
            ),
            (StagingOp::CreateLibrary { library_title, .. }, Lang::Es) => {
                format!("[+Bib] Crear biblioteca '{}'", library_title)
            }
            (StagingOp::CreateLibrary { library_title, .. }, Lang::En) => {
                format!("[+Bib] Create library '{}'", library_title)
            }
            (StagingOp::CreateGroup { group_title, .. }, Lang::Es) => {
                format!("[+Grp] Crear grupo SharePoint '{}'", group_title)
            }
            (StagingOp::CreateGroup { group_title, .. }, Lang::En) => {
                format!("[+Grp] Create SharePoint group '{}'", group_title)
            }
            (
                StagingOp::CreateFolder {
                    library_title,
                    relative_folder_path,
                    ..
                },
                Lang::Es,
            ) => format!(
                "[+Dir] Crear carpeta '{}/{}'",
                library_title, relative_folder_path
            ),
            (
                StagingOp::CreateFolder {
                    library_title,
                    relative_folder_path,
                    ..
                },
                Lang::En,
            ) => format!(
                "[+Dir] Create folder '{}/{}'",
                library_title, relative_folder_path
            ),
            (
                StagingOp::BreakFolderInheritance {
                    library_title,
                    relative_folder_path,
                    copy_parent_roles,
                    clear_subscopes,
                    ..
                },
                Lang::Es,
            ) => format!(
                "[Perm] Romper herencia en '{}/{}' (copy={}, clear={})",
                library_title, relative_folder_path, copy_parent_roles, clear_subscopes
            ),
            (
                StagingOp::BreakFolderInheritance {
                    library_title,
                    relative_folder_path,
                    copy_parent_roles,
                    clear_subscopes,
                    ..
                },
                Lang::En,
            ) => format!(
                "[Perm] Break inheritance on '{}/{}' (copy={}, clear={})",
                library_title, relative_folder_path, copy_parent_roles, clear_subscopes
            ),
            (
                StagingOp::AssignFolderRole {
                    library_title,
                    relative_folder_path,
                    principal_name,
                    role_name,
                    ..
                },
                Lang::Es,
            ) => format!(
                "[+Rol] Asignar '{}' a '{}' en '{}/{}'",
                role_name, principal_name, library_title, relative_folder_path
            ),
            (
                StagingOp::AssignFolderRole {
                    library_title,
                    relative_folder_path,
                    principal_name,
                    role_name,
                    ..
                },
                Lang::En,
            ) => format!(
                "[+Rol] Grant '{}' to '{}' on '{}/{}'",
                role_name, principal_name, library_title, relative_folder_path
            ),
            (
                StagingOp::RemoveFolderRole {
                    library_title,
                    relative_folder_path,
                    principal_name,
                    role_name,
                    ..
                },
                Lang::Es,
            ) => format!(
                "[-Rol] Revocar '{}' de '{}' en '{}/{}'",
                role_name, principal_name, library_title, relative_folder_path
            ),
            (
                StagingOp::RemoveFolderRole {
                    library_title,
                    relative_folder_path,
                    principal_name,
                    role_name,
                    ..
                },
                Lang::En,
            ) => format!(
                "[-Rol] Revoke '{}' from '{}' on '{}/{}'",
                role_name, principal_name, library_title, relative_folder_path
            ),
            (
                StagingOp::ResetFolderInheritance {
                    library_title,
                    relative_folder_path,
                    ..
                },
                Lang::Es,
            ) => format!(
                "[Herencia] Restaurar herencia del padre en '{}/{}'",
                library_title, relative_folder_path
            ),
            (
                StagingOp::ResetFolderInheritance {
                    library_title,
                    relative_folder_path,
                    ..
                },
                Lang::En,
            ) => format!(
                "[Inherit] Restore parent inheritance on '{}/{}'",
                library_title, relative_folder_path
            ),
            (
                StagingOp::AssignSiteRole {
                    group_title,
                    role_name,
                    ..
                },
                Lang::Es,
            ) => format!(
                "[+SitioRol] Asignar '{}' al grupo '{}' en el sitio",
                role_name, group_title
            ),
            (
                StagingOp::AssignSiteRole {
                    group_title,
                    role_name,
                    ..
                },
                Lang::En,
            ) => format!(
                "[+SitioRol] Grant '{}' to group '{}' on site",
                role_name, group_title
            ),
            (
                StagingOp::ServerSideCopy {
                    source_server_relative_url,
                    target_folder_server_relative_url,
                    is_folder,
                    ..
                },
                Lang::Es,
            ) => {
                let kind = if *is_folder { "Carpeta" } else { "Archivo" };
                format!(
                    "[Copy {}] '{}' -> '{}'",
                    kind, source_server_relative_url, target_folder_server_relative_url
                )
            }
            (
                StagingOp::ServerSideCopy {
                    source_server_relative_url,
                    target_folder_server_relative_url,
                    is_folder,
                    ..
                },
                Lang::En,
            ) => {
                let kind = if *is_folder { "Folder" } else { "File" };
                format!(
                    "[Copy {}] '{}' -> '{}'",
                    kind, source_server_relative_url, target_folder_server_relative_url
                )
            }
            (
                StagingOp::SetSiteOfflineSync {
                    site_title,
                    exclude_from_offline_client,
                    ..
                },
                Lang::Es,
            ) => {
                if *exclude_from_offline_client {
                    format!(
                        "[-Sync] Bloquear sincronización SharePoint en el sitio '{}'",
                        site_title
                    )
                } else {
                    format!(
                        "[+Sync] Permitir sincronización SharePoint en el sitio '{}'",
                        site_title
                    )
                }
            }
            (
                StagingOp::SetSiteOfflineSync {
                    site_title,
                    exclude_from_offline_client,
                    ..
                },
                Lang::En,
            ) => {
                if *exclude_from_offline_client {
                    format!(
                        "[-Sync] Block SharePoint sync on site '{}'",
                        site_title
                    )
                } else {
                    format!(
                        "[+Sync] Allow SharePoint sync on site '{}'",
                        site_title
                    )
                }
            }
            (
                StagingOp::SetLibraryOfflineSync {
                    library_title,
                    exclude_from_offline_client,
                    apply_view_formatting,
                    ..
                },
                Lang::Es,
            ) => match (*exclude_from_offline_client, *apply_view_formatting) {
                (true, true) => format!(
                    "[-Sync] Desactivar sync offline + ocultar acceso directo OneDrive en '{}'",
                    library_title
                ),
                (true, false) => format!(
                    "[-Sync] Bloquear sincronización SharePoint en biblioteca '{}'",
                    library_title
                ),
                (false, true) => format!(
                    "[+Sync] Activar sync offline + mostrar acceso directo OneDrive en '{}'",
                    library_title
                ),
                (false, false) => format!(
                    "[+Sync] Permitir sincronización SharePoint en biblioteca '{}'",
                    library_title
                ),
            },
            (
                StagingOp::SetLibraryOfflineSync {
                    library_title,
                    exclude_from_offline_client,
                    apply_view_formatting,
                    ..
                },
                Lang::En,
            ) => match (*exclude_from_offline_client, *apply_view_formatting) {
                (true, true) => format!(
                    "[-Sync] Disable offline sync + hide OneDrive shortcut on '{}'",
                    library_title
                ),
                (true, false) => format!(
                    "[-Sync] Block SharePoint sync on library '{}'",
                    library_title
                ),
                (false, true) => format!(
                    "[+Sync] Enable offline sync + show OneDrive shortcut on '{}'",
                    library_title
                ),
                (false, false) => format!(
                    "[+Sync] Allow SharePoint sync on library '{}'",
                    library_title
                ),
            },
            (
                StagingOp::SetLibraryOneDriveShortcuts {
                    library_title,
                    hide_shortcut_button,
                    ..
                },
                Lang::Es,
            ) => {
                if *hide_shortcut_button {
                    format!(
                        "[-AtajoOD] Ocultar acceso directo a OneDrive en biblioteca '{}'",
                        library_title
                    )
                } else {
                    format!(
                        "[+AtajoOD] Mostrar acceso directo a OneDrive en biblioteca '{}'",
                        library_title
                    )
                }
            }
            (
                StagingOp::SetLibraryOneDriveShortcuts {
                    library_title,
                    hide_shortcut_button,
                    ..
                },
                Lang::En,
            ) => {
                if *hide_shortcut_button {
                    format!(
                        "[-AtajoOD] Hide OneDrive shortcut on library '{}'",
                        library_title
                    )
                } else {
                    format!(
                        "[+AtajoOD] Show OneDrive shortcut on library '{}'",
                        library_title
                    )
                }
            }
            (StagingOp::DeleteLibrary { library_title, .. }, Lang::Es) => {
                format!("[-DEL Bib] Eliminar biblioteca '{}'", library_title)
            }
            (StagingOp::DeleteLibrary { library_title, .. }, Lang::En) => {
                format!("[-DEL Bib] Delete library '{}'", library_title)
            }
            (
                StagingOp::DeleteFolder {
                    folder_server_relative_url,
                    ..
                },
                Lang::Es,
            ) => format!("[-DEL Dir] Eliminar carpeta '{}'", folder_server_relative_url),
            (
                StagingOp::DeleteFolder {
                    folder_server_relative_url,
                    ..
                },
                Lang::En,
            ) => format!("[-DEL Dir] Delete folder '{}'", folder_server_relative_url),
            (
                StagingOp::DeleteFile {
                    file_server_relative_url,
                    ..
                },
                Lang::Es,
            ) => format!("[-DEL File] Eliminar archivo '{}'", file_server_relative_url),
            (
                StagingOp::DeleteFile {
                    file_server_relative_url,
                    ..
                },
                Lang::En,
            ) => format!("[-DEL File] Delete file '{}'", file_server_relative_url),
            (StagingOp::DeleteGroup { group_title, .. }, Lang::Es) => {
                format!("[-DEL Grp] Eliminar grupo SharePoint '{}'", group_title)
            }
            (StagingOp::DeleteGroup { group_title, .. }, Lang::En) => {
                format!("[-DEL Grp] Delete SharePoint group '{}'", group_title)
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
    RestoreFolderRoleAssignment {
        site_url: String,
        folder_server_relative_url: String,
        principal_id: i64,
        role_def_id: i64,
    },
    RestoreFolderUniqueRoles {
        site_url: String,
        folder_server_relative_url: String,
        had_unique_roles_before: bool,
        #[serde(default)]
        previous_assignments: Vec<FolderPermissionEntry>,
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
        #[serde(default = "default_true")]
        restore_exclude_flag: bool,
        #[serde(default)]
        previous_view_formatters: Vec<ViewFormatterBackup>,
    },
    RestoreLibraryOneDriveShortcuts {
        site_url: String,
        library_id: String,
        #[serde(default)]
        previous_view_formatters: Vec<ViewFormatterBackup>,
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

/// Mode for Phase 3 site cloning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CloneMode {
    #[default]
    Additive,
    Destructive,
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
    #[serde(default)]
    pub mode: CloneMode,
    #[serde(default)]
    pub current_actor_upn: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Lang;

    #[test]
    fn test_folder_inheritance_summary_and_impact_analysis() {
        let owners = FolderPermissionEntry {
            principal_id: 3,
            principal_title: "Site Owners".to_string(),
            principal_login_name: "Site Owners".to_string(),
            principal_type: "SharePointGroup".to_string(),
            roles: vec!["Full Control".to_string()],
        };
        let members = FolderPermissionEntry {
            principal_id: 5,
            principal_title: "Site Members".to_string(),
            principal_login_name: "Site Members".to_string(),
            principal_type: "SharePointGroup".to_string(),
            roles: vec!["Edit".to_string()],
        };

        // Chain: Library (root) -> L1 (unique: Owners + Members=Edit) -> L2 (inherits) -> L3 (inherits)
        let ancestors = vec![
            FolderAncestorInfo {
                relative_path: String::new(),
                name: "Documentos".to_string(),
                server_relative_url: "/sites/Demo/Shared Documents".to_string(),
                has_unique_role_assignments: false,
                role_assignments: vec![owners.clone(), members.clone()],
            },
            FolderAncestorInfo {
                relative_path: "01_Dir".to_string(),
                name: "01_Dir".to_string(),
                server_relative_url: "/sites/Demo/Shared Documents/01_Dir".to_string(),
                has_unique_role_assignments: true,
                role_assignments: vec![owners.clone(), members.clone()],
            },
            FolderAncestorInfo {
                relative_path: "01_Dir/Sub".to_string(),
                name: "Sub".to_string(),
                server_relative_url: "/sites/Demo/Shared Documents/01_Dir/Sub".to_string(),
                has_unique_role_assignments: false,
                role_assignments: vec![owners.clone(), members.clone()],
            },
        ];

        let summary = FolderInheritanceSummary::from_ancestors(&ancestors, Lang::Es);
        assert_eq!(summary.inherited_from_path, "01_Dir");
        assert_eq!(summary.inherited_from_label, "01_Dir");
        assert!(summary.source_has_unique_roles);
        assert_eq!(summary.inherited_roles.len(), 2);

        // Falls back to library root when no ancestor has unique roles
        let root_only = vec![ancestors[0].clone()];
        let root_summary = FolderInheritanceSummary::from_ancestors(&root_only, Lang::En);
        assert_eq!(root_summary.inherited_from_path, "");
        assert_eq!(root_summary.inherited_from_label, "Library 'Documentos'");

        // Downgrading Site Members from Edit to Read with copy_parent_roles=true detects conflicting Edit role
        let impact_downgrade = analyze_permission_change_impact(
            false,
            &[],
            &summary,
            "Site Members",
            "Read",
            true,
            false,
        );
        assert_eq!(impact_downgrade.conflicting_roles, vec!["Edit".to_string()]);
        assert!(!impact_downgrade.will_lose_parent_roles);
        assert!(!impact_downgrade.triggers_limited_access_upward);
        assert!(impact_downgrade.disconnects_from_parent);

        // Granting to a brand new group with copy_parent_roles=false and clear_subscopes=true
        let impact_new = analyze_permission_change_impact(
            false,
            &[],
            &summary,
            "Equipo_Externo",
            "Read",
            false,
            true,
        );
        assert!(impact_new.conflicting_roles.is_empty());
        assert!(impact_new.will_lose_parent_roles);
        assert!(impact_new.triggers_limited_access_upward);
        assert!(impact_new.wipes_child_unique_scopes);
    }
}
