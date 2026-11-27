use crate::api::graph::GraphClient;
use crate::api::spo::SpoClient;
use crate::auth::{AuthManager, DeviceCodePrompt};
use crate::db::Database;
use crate::i18n::Lang;
use crate::models::{
    DirectoryUser, FolderAncestorInfo, FolderInheritanceSummary,
    FolderPermissionEntry, SpoFolder, SpoGroup, SpoLibrary, SpoSite, SpoUser, StagingOp,
    TransactionOpRecord, TransactionRecord,
};
use crate::staging::{ExecutionReport, StagingQueue};
use std::collections::HashSet;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};


mod actions;
mod sync;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Auth,
    SiteSelector,
    SiteWorkspace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceTab {
    GroupsAndUsers,      // Tab 1 (Phase 1)
    LibrariesAndFolders, // Tab 2 (Phase 2)
    CloneSite,           // Tab 3 (Phase 3)
    StagingAndHistory,   // Tab 4 (Staging + SQLite History & Rollback)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneFocus {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMenuScope {
    SelectedLibrary,
    SiteAndLibraries,
}

#[derive(Debug, Clone)]
pub enum Modal {
    None,
    Help,
    TextInput {
        title: String,
        prompt: String,
        value: String,
        kind: TextInputKind,
    },
    AddUserToGroup {
        group_id: i64,
        group_title: String,
        query: String,
        results: Vec<DirectoryUser>,
        selected_idx: usize,
        marked_identifiers: HashSet<String>,
        searching: bool,
    },
    GrantFolderPermission {
        library_title: String,
        relative_folder_path: String,
        target_has_unique_roles: bool,
        target_current_roles: Vec<FolderPermissionEntry>,
        inheritance: FolderInheritanceSummary,
        group_name: String,
        role_idx: usize, // 0=Read, 1=Contribute, 2=Edit, 3=Full Control
        copy_parent_roles: bool,
        replace_inherited_roles: bool,
        clear_subscopes: bool,
        create_group_if_missing: bool,
        active_field: usize, // 0=group_name, 1=role, 2=copy_parent, 3=replace_inherited, 4=clear_subscopes
    },
    ManageFolderPermissions {
        library_title: String,
        relative_folder_path: String,
        target_has_unique_roles: bool,
        inheritance: FolderInheritanceSummary,
        entries: Vec<(String, String, String)>, // (principal_title, principal_type, role_name)
        selected_idx: usize,
    },
    ConfirmResetFolderInheritance {
        library_title: String,
        relative_folder_path: String,
        inheritance: FolderInheritanceSummary,
    },
    ConfirmApplyStaging {
        note: String,
    },
    ConfirmRevertTransaction {
        tx_id: i64,
        only_op_id: Option<i64>,
        summary: String,
    },
    SyncControlMenu {
        scope: SyncMenuScope,
        selected_idx: usize, // 0 = Sincronización SharePoint, 1 = Accesos directos OneDrive, 2 = Ambos
    },
    ConfirmDestructiveClone {
        source_url: String,
        target_url: String,
        typed_confirm: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextInputKind {
    FilterSitesLocal,
    SearchSitesTenant,
    ManualSiteUrl,
    FilterCurrentPane,
    CreateSharePointGroup,
    CreateDocumentLibrary,
    CreateFolderRelativePath,
    ImportMatrixFilePath,
    ExportMatrixFilePath,
    SetCloneSourceUrl,
}

pub const STANDARD_ROLES: &[&str] = &["Read", "Contribute", "Edit", "Full Control"];

pub enum AsyncEvent {
    AuthCompleted {
        user_upn: String,
        display_name: String,
        root_url: String,
    },
    DeviceCodeReady(DeviceCodePrompt),
    SitesDiscovered(Vec<SpoSite>),
    SingleSiteChecked(SpoSite),
    SiteWorkspaceLoaded {
        groups: Vec<SpoGroup>,
        initial_members: Vec<SpoUser>,
        libraries: Vec<SpoLibrary>,
        loaded_library_title: Option<String>,
        loaded_folder_path: String,
        initial_ancestors: Vec<FolderAncestorInfo>,
        initial_folders: Vec<SpoFolder>,
        transactions: Vec<TransactionRecord>,
        site_exclude_from_offline_client: Option<bool>,
    },
    GroupMembersLoaded {
        group_id: i64,
        members: Vec<SpoUser>,
    },
    LibraryFoldersLoaded {
        library_title: String,
        relative_subpath: String,
        ancestors: Vec<FolderAncestorInfo>,
        folders: Vec<SpoFolder>,
        select_folder_name: Option<String>,
    },
    DirectoryUsersFound {
        query: String,
        results: Vec<DirectoryUser>,
    },
    MatrixImported {
        queue_ops: Vec<StagingOp>,
        source_file: String,
    },
    MatrixExported {
        count: usize,
        path: String,
    },
    ClonePlanned {
        queue_ops: Vec<StagingOp>,
        source_url: String,
    },
    StagingApplied {
        report: ExecutionReport,
    },
    TransactionReverted {
        tx: TransactionRecord,
        message: String,
    },
    TransactionsLoaded {
        transactions: Vec<TransactionRecord>,
        selected_ops: Vec<TransactionOpRecord>,
    },
    TransactionOpsLoaded {
        tx_id: i64,
        ops: Vec<TransactionOpRecord>,
    },
    Error(String),
}

pub struct App {
    pub lang: Lang,
    pub should_quit: bool,
    pub screen: Screen,
    pub tab: WorkspaceTab,
    pub focus: PaneFocus,
    pub modal: Modal,

    pub auth: AuthManager,
    pub db: Database,
    pub graph: GraphClient,
    pub spo: SpoClient,

    pub tx_sender: UnboundedSender<AsyncEvent>,
    pub rx_receiver: UnboundedReceiver<AsyncEvent>,

    pub busy_message: Option<String>,
    pub status_toast: Option<(String, bool)>, // (message, is_error)

    // Auth state
    pub user_upn: String,
    pub user_display_name: String,
    pub root_site_url: String,
    pub device_code_prompt: Option<DeviceCodePrompt>,
    pub login_email_input: String,

    // Site selector state
    pub sites: Vec<SpoSite>,
    pub site_filter: String,
    pub selected_site_idx: usize,
    pub current_site: Option<SpoSite>,
    pub site_exclude_from_offline_client: Option<bool>,

    // Tab 1: Groups & Users state
    pub groups: Vec<SpoGroup>,
    pub selected_group_idx: usize,
    pub group_members: Vec<SpoUser>,
    pub selected_member_idx: usize,
    pub pane_filter: String,

    // Tab 2: Libraries & Folders state
    pub libraries: Vec<SpoLibrary>,
    pub selected_library_idx: usize,
    pub current_folder_path: String, // "" = library root; "Dir/Sub" = nested folder
    pub folder_ancestors: Vec<FolderAncestorInfo>,
    pub folders: Vec<SpoFolder>,
    pub selected_folder_idx: usize,

    // Tab 3: Site Clone state
    pub clone_source_url: String,
    pub clone_target_base_folder: String,
    pub clone_include_groups: bool,
    pub clone_include_files: bool,
    pub clone_include_unique_perms: bool,
    pub clone_overwrite: bool,
    pub clone_destructive: bool,
    pub clone_selected_option: usize,

    // Tab 4: Staging & SQLite Transactions state
    pub staging: StagingQueue,
    pub selected_staging_idx: usize,
    pub last_staging_source: String,
    pub transactions: Vec<TransactionRecord>,
    pub selected_tx_idx: usize,
    pub selected_tx_ops: Vec<TransactionOpRecord>,
    pub selected_tx_op_idx: usize,

    // Per-panel vertical scroll offsets updated dynamically during rendering
    pub scroll: ScrollOffsets,
}

#[derive(Debug, Default, Clone)]
pub struct ScrollOffsets {
    pub sites: std::cell::Cell<usize>,
    pub groups: std::cell::Cell<usize>,
    pub members: std::cell::Cell<usize>,
    pub libraries: std::cell::Cell<usize>,
    pub folders: std::cell::Cell<usize>,
    pub staging: std::cell::Cell<usize>,
    pub transactions: std::cell::Cell<usize>,
    pub tx_ops: std::cell::Cell<usize>,
    pub modal_users: std::cell::Cell<usize>,
    pub modal_folder_perms: std::cell::Cell<usize>,
}

impl App {
    pub fn new(
        auth: AuthManager,
        db: Database,
        graph: GraphClient,
        spo: SpoClient,
        lang: Lang,
    ) -> Self {
        let (tx_sender, rx_receiver) = mpsc::unbounded_channel();
        let cached_sites = db.load_cached_sites().unwrap_or_default();
        let sess = auth.load_session().ok().flatten();
        let login_email_input = auth.detect_default_login_hint().unwrap_or_default();

        let (initial_screen, upn, disp, root) = if let Some(s) = sess {
            (
                Screen::SiteSelector,
                s.user_upn.unwrap_or_else(|| "usuario@m365".to_string()),
                s.user_display_name
                    .unwrap_or_else(|| lang.tr("Usuario M365", "M365 User").to_string()),
                s.root_site_url.unwrap_or_default(),
            )
        } else {
            (Screen::Auth, String::new(), String::new(), String::new())
        };

        let mut app = Self {
            lang,
            should_quit: false,
            screen: initial_screen,
            tab: WorkspaceTab::GroupsAndUsers,
            focus: PaneFocus::Left,
            modal: Modal::None,
            auth,
            db,
            graph,
            spo,
            tx_sender,
            rx_receiver,
            busy_message: None,
            status_toast: None,
            user_upn: upn,
            user_display_name: disp,
            root_site_url: root,
            device_code_prompt: None,
            login_email_input,
            sites: cached_sites,
            site_filter: String::new(),
            selected_site_idx: 0,
            current_site: None,
            site_exclude_from_offline_client: None,
            groups: Vec::new(),
            selected_group_idx: 0,
            group_members: Vec::new(),
            selected_member_idx: 0,
            pane_filter: String::new(),
            libraries: Vec::new(),
            selected_library_idx: 0,
            current_folder_path: String::new(),
            folder_ancestors: Vec::new(),
            folders: Vec::new(),
            selected_folder_idx: 0,
            clone_source_url: String::new(),
            clone_target_base_folder: String::new(),
            clone_include_groups: true,
            clone_include_files: true,
            clone_include_unique_perms: true,
            clone_overwrite: true,
            clone_destructive: false,
            clone_selected_option: 0,
            staging: StagingQueue::new(),
            selected_staging_idx: 0,
            last_staging_source: "TUI".to_string(),
            transactions: Vec::new(),
            selected_tx_idx: 0,
            selected_tx_ops: Vec::new(),
            selected_tx_op_idx: 0,
            scroll: ScrollOffsets::default(),
        };

        if app.screen == Screen::SiteSelector {
            app.trigger_discover_sites(None);
        }

        app
    }

    pub fn filtered_sites(&self) -> Vec<&SpoSite> {
        let q = self.site_filter.trim().to_lowercase();
        self.sites
            .iter()
            .filter(|s| {
                q.is_empty()
                    || s.title.to_lowercase().contains(&q)
                    || s.url.to_lowercase().contains(&q)
                    || s.role.badge().to_lowercase().contains(&q)
            })
            .collect()
    }

    pub fn selected_site(&self) -> Option<SpoSite> {
        let list = self.filtered_sites();
        list.get(self.selected_site_idx).map(|s| (*s).clone())
    }

    pub fn selected_group(&self) -> Option<&SpoGroup> {
        self.groups.get(self.selected_group_idx)
    }

    pub fn selected_library(&self) -> Option<&SpoLibrary> {
        self.libraries.get(self.selected_library_idx)
    }

    pub fn is_in_subfolder(&self) -> bool {
        !self.current_folder_path.is_empty()
    }

    pub fn folder_list_row_count(&self) -> usize {
        self.folders.len() + usize::from(self.is_in_subfolder())
    }

    pub fn is_parent_row_selected(&self) -> bool {
        self.is_in_subfolder() && self.selected_folder_idx == 0
    }

    pub fn selected_folder(&self) -> Option<&SpoFolder> {
        if self.is_in_subfolder() {
            if self.selected_folder_idx == 0 {
                None
            } else {
                self.folders.get(self.selected_folder_idx - 1)
            }
        } else {
            self.folders.get(self.selected_folder_idx)
        }
    }

    pub fn current_level_inheritance(&self) -> FolderInheritanceSummary {
        FolderInheritanceSummary::from_ancestors(&self.folder_ancestors, self.lang)
    }

    /// Resolve the target folder context for permission operations (`p`, `d`, `x`):
    /// uses the highlighted child folder if one is selected, or the current folder itself
    /// when `..` is highlighted inside a subfolder.
    pub fn target_folder_context(
        &self,
    ) -> Option<(
        String,
        String,
        bool,
        Vec<FolderPermissionEntry>,
        FolderInheritanceSummary,
    )> {
        let lib_title = self
            .selected_library()
            .map(|l| l.title.clone())
            .unwrap_or_else(|| "Documents".to_string());

        if let Some(folder) = self.selected_folder() {
            let inheritance = self.current_level_inheritance();
            return Some((
                lib_title,
                folder.relative_path.clone(),
                folder.has_unique_role_assignments,
                folder.role_assignments.clone(),
                inheritance,
            ));
        }

        if self.is_in_subfolder() {
            if let Some(cur) = self.folder_ancestors.last() {
                let parent_slice =
                    &self.folder_ancestors[..self.folder_ancestors.len().saturating_sub(1)];
                let parent_inh = FolderInheritanceSummary::from_ancestors(parent_slice, self.lang);
                return Some((
                    lib_title,
                    cur.relative_path.clone(),
                    cur.has_unique_role_assignments,
                    cur.role_assignments.clone(),
                    parent_inh,
                ));
            }
        }

        None
    }

    fn resolve_login_target_or_toast(&mut self) -> Option<String> {
        let input = self.login_email_input.trim().to_string();
        let target = if input.is_empty() {
            self.auth.detect_default_login_hint()
        } else {
            Some(input)
        };
        if target.is_none() {
            self.status_toast = Some((
                self.lang
                    .tr(
                        "Introduce tu correo corporativo M365 (ej. usuario@dominio.com) antes de iniciar sesión",
                        "Enter your M365 corporate email (e.g. user@domain.com) before signing in",
                    )
                    .to_string(),
                true,
            ));
        }
        target
    }


    pub fn refresh_transactions_from_db(&mut self) {
        let filter = self.current_site.as_ref().map(|s| s.url.as_str());
        self.transactions = self.db.list_transactions(filter, 50).unwrap_or_default();
        if self.selected_tx_idx >= self.transactions.len() {
            self.selected_tx_idx = 0;
        }
        if let Some(first) = self.transactions.get(self.selected_tx_idx) {
            self.selected_tx_ops = self
                .db
                .get_transaction_operations(first.id)
                .unwrap_or_default();
        } else {
            self.selected_tx_ops.clear();
        }
        self.selected_tx_op_idx = 0;
    }

    pub fn handle_async_event(&mut self, ev: AsyncEvent) {
        match ev {
            AsyncEvent::AuthCompleted {
                user_upn,
                display_name,
                root_url,
            } => {
                self.busy_message = None;
                self.device_code_prompt = None;
                self.user_upn = user_upn;
                self.user_display_name = display_name;
                self.root_site_url = root_url;
                self.screen = Screen::SiteSelector;
                self.status_toast = Some((
                    self.lang
                        .tr(
                            "✓ Sesión iniciada en Microsoft 365",
                            "✓ Signed in to Microsoft 365",
                        )
                        .to_string(),
                    false,
                ));
                self.trigger_discover_sites(None);
            }
            AsyncEvent::DeviceCodeReady(prompt) => {
                self.busy_message = Some(match self.lang {
                    Lang::Es => format!(
                        "Esperando confirmación del código {} en {}",
                        prompt.user_code, prompt.verification_uri
                    ),
                    Lang::En => format!(
                        "Waiting for code {} confirmation at {}",
                        prompt.user_code, prompt.verification_uri
                    ),
                });
                self.device_code_prompt = Some(prompt);
            }
            AsyncEvent::SitesDiscovered(sites) => {
                self.busy_message = None;
                let count = sites.len();
                self.sites = sites;
                self.selected_site_idx = 0;
                self.status_toast = Some((
                    match self.lang {
                        Lang::Es => format!(
                            "✓ {} sitio(s) SharePoint cargados donde eres Admin o Propietario",
                            count
                        ),
                        Lang::En => format!(
                            "✓ Loaded {} SharePoint site(s) where you are Admin or Owner",
                            count
                        ),
                    },
                    false,
                ));
            }
            AsyncEvent::SingleSiteChecked(site) => {
                self.busy_message = None;
                if let Some(pos) = self
                    .sites
                    .iter()
                    .position(|s| s.url.eq_ignore_ascii_case(&site.url))
                {
                    self.sites[pos] = site.clone();
                } else {
                    self.sites.insert(0, site.clone());
                }
                self.enter_site_workspace(site);
            }
            AsyncEvent::SiteWorkspaceLoaded {
                groups,
                initial_members,
                libraries,
                loaded_library_title,
                loaded_folder_path,
                initial_ancestors,
                initial_folders,
                transactions,
                site_exclude_from_offline_client,
            } => {
                self.busy_message = None;
                self.groups = groups;
                self.group_members = initial_members;
                self.libraries = libraries;
                if let Some(ref title) = loaded_library_title {
                    if let Some(idx) = self
                        .libraries
                        .iter()
                        .position(|l| l.title.eq_ignore_ascii_case(title))
                    {
                        self.selected_library_idx = idx;
                    }
                }
                self.current_folder_path = loaded_folder_path;
                self.folder_ancestors = initial_ancestors;
                self.folders = initial_folders;
                if self.selected_folder_idx >= self.folder_list_row_count() {
                    self.selected_folder_idx = 0;
                }
                self.transactions = transactions;
                self.site_exclude_from_offline_client = site_exclude_from_offline_client;
                if let Some(first_tx) = self.transactions.first() {
                    self.selected_tx_ops = self
                        .db
                        .get_transaction_operations(first_tx.id)
                        .unwrap_or_default();
                } else {
                    self.selected_tx_ops.clear();
                }
            }
            AsyncEvent::GroupMembersLoaded { group_id, members } => {
                self.busy_message = None;
                if self.selected_group().map(|g| g.id) == Some(group_id) {
                    self.group_members = members;
                    self.selected_member_idx = 0;
                }
            }
            AsyncEvent::LibraryFoldersLoaded {
                library_title,
                relative_subpath,
                ancestors,
                folders,
                select_folder_name,
            } => {
                self.busy_message = None;
                if self.selected_library().map(|l| l.title.as_str()) == Some(library_title.as_str())
                    && self.current_folder_path == relative_subpath
                {
                    self.folder_ancestors = ancestors;
                    self.folders = folders;
                    let parent_offset = usize::from(self.is_in_subfolder());
                    self.selected_folder_idx = select_folder_name
                        .as_deref()
                        .and_then(|name| {
                            self.folders
                                .iter()
                                .position(|f| f.name.eq_ignore_ascii_case(name))
                        })
                        .map(|i| i + parent_offset)
                        .unwrap_or(0);
                }
            }
            AsyncEvent::DirectoryUsersFound { query, results } => {
                if let Modal::AddUserToGroup {
                    query: ref modal_query,
                    results: ref mut modal_res,
                    selected_idx: ref mut sel,
                    searching: ref mut srch,
                    ..
                } = self.modal
                {
                    if modal_query.trim().eq_ignore_ascii_case(query.trim()) {
                        *modal_res = results;
                        *sel = 0;
                        *srch = false;
                    }
                }
            }
            AsyncEvent::MatrixImported {
                queue_ops,
                source_file,
            } => {
                self.busy_message = None;
                let count = queue_ops.len();
                for op in queue_ops {
                    self.staging.push(op);
                }
                self.last_staging_source = format!("MATRIX_IMPORT:{}", source_file);
                self.tab = WorkspaceTab::StagingAndHistory;
                self.focus = PaneFocus::Left;
                self.status_toast = Some((
                    match self.lang {
                        Lang::Es => format!(
                            "✓ Matriz '{}' cargada: {} operación(es) añadidas a Staging. Pulsa 'A' para Aplicar.",
                            source_file, count
                        ),
                        Lang::En => format!(
                            "✓ Matrix '{}' loaded: {} operation(s) queued in Staging. Press 'A' to Apply.",
                            source_file, count
                        ),
                    },
                    false,
                ));
            }
            AsyncEvent::MatrixExported { count, path } => {
                self.busy_message = None;
                self.status_toast = Some((
                    match self.lang {
                        Lang::Es => format!(
                            "✓ Exportadas {} carpetas con permisos únicos en '{}'",
                            count, path
                        ),
                        Lang::En => format!(
                            "✓ Exported {} folder(s) with unique permissions to '{}'",
                            count, path
                        ),
                    },
                    false,
                ));
            }
            AsyncEvent::ClonePlanned {
                queue_ops,
                source_url,
            } => {
                self.busy_message = None;
                let count = queue_ops.len();
                for op in queue_ops {
                    self.staging.push(op);
                }
                self.last_staging_source = format!("SITE_CLONE:{}", source_url);
                self.tab = WorkspaceTab::StagingAndHistory;
                self.focus = PaneFocus::Left;
                self.status_toast = Some((
                    match self.lang {
                        Lang::Es => format!(
                            "✓ Plan de clonado listo ({} operaciones en Staging). Revisa y pulsa 'A' para Aplicar.",
                            count
                        ),
                        Lang::En => format!(
                            "✓ Clone plan ready ({} operation(s) in Staging). Review and press 'A' to Apply.",
                            count
                        ),
                    },
                    false,
                ));
            }
            AsyncEvent::StagingApplied { report } => {
                self.busy_message = None;
                self.staging.clear();
                self.last_staging_source = "TUI".to_string();
                let is_err = report.status == "FAILED";
                self.status_toast = Some((
                    match self.lang {
                        Lang::Es => format!(
                            "✓ Transacción #{:?} [{}]: {}",
                            report.transaction_id.unwrap_or(0),
                            report.status,
                            report.summary
                        ),
                        Lang::En => format!(
                            "✓ Transaction #{:?} [{}]: {}",
                            report.transaction_id.unwrap_or(0),
                            report.status,
                            report.summary
                        ),
                    },
                    is_err,
                ));
                if let Some(site_url) = self.current_site.as_ref().map(|s| s.url.clone()) {
                    self.trigger_reload_workspace(&site_url);
                }
            }
            AsyncEvent::TransactionReverted { tx: _, message } => {
                self.busy_message = None;
                self.status_toast = Some((format!("✓ {}", message), false));
                self.refresh_transactions_from_db();
                if let Some(site_url) = self.current_site.as_ref().map(|s| s.url.clone()) {
                    self.trigger_reload_workspace(&site_url);
                }
            }
            AsyncEvent::TransactionsLoaded {
                transactions,
                selected_ops,
            } => {
                self.busy_message = None;
                self.transactions = transactions;
                self.selected_tx_ops = selected_ops;
            }
            AsyncEvent::TransactionOpsLoaded { tx_id, ops } => {
                if self.transactions.get(self.selected_tx_idx).map(|t| t.id) == Some(tx_id) {
                    self.selected_tx_ops = ops;
                    self.selected_tx_op_idx = 0;
                }
            }
            AsyncEvent::Error(err) => {
                self.busy_message = None;
                self.status_toast = Some((format!("✗ {}", err), true));
            }
        }
    }
}
