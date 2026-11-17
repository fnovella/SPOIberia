use crate::api::graph::GraphClient;
use crate::api::spo::SpoClient;
use crate::auth::{AuthManager, DeviceCodePrompt};
use crate::cloner::SiteClonerEngine;
use crate::db::Database;
use crate::importer::MatrixImporter;
use crate::models::{
    CloneConfig, DirectoryUser, SpoFolder, SpoGroup, SpoLibrary, SpoSite, SpoUser, StagingOp,
    TransactionOpRecord, TransactionRecord,
};
use crate::staging::{ExecutionReport, StagingQueue, TransactionExecutor};
use std::collections::HashSet;
use std::path::PathBuf;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

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
        group_name: String,
        role_idx: usize, // 0=Read, 1=Contribute, 2=Edit, 3=Full Control
        copy_parent_roles: bool,
        clear_subscopes: bool,
        create_group_if_missing: bool,
        active_field: usize, // 0=group_name, 1=role, 2=copy_parent, 3=clear_subscopes
    },
    ConfirmApplyStaging {
        note: String,
    },
    ConfirmRevertTransaction {
        tx_id: i64,
        only_op_id: Option<i64>,
        summary: String,
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
        initial_folders: Vec<SpoFolder>,
        transactions: Vec<TransactionRecord>,
    },
    GroupMembersLoaded {
        group_id: i64,
        members: Vec<SpoUser>,
    },
    LibraryFoldersLoaded {
        library_title: String,
        folders: Vec<SpoFolder>,
    },
    DirectoryUsersFound(Vec<DirectoryUser>),
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

    // Site selector state
    pub sites: Vec<SpoSite>,
    pub site_filter: String,
    pub selected_site_idx: usize,
    pub current_site: Option<SpoSite>,

    // Tab 1: Groups & Users state
    pub groups: Vec<SpoGroup>,
    pub selected_group_idx: usize,
    pub group_members: Vec<SpoUser>,
    pub selected_member_idx: usize,
    pub pane_filter: String,

    // Tab 2: Libraries & Folders state
    pub libraries: Vec<SpoLibrary>,
    pub selected_library_idx: usize,
    pub folders: Vec<SpoFolder>,
    pub selected_folder_idx: usize,

    // Tab 3: Site Clone state
    pub clone_source_url: String,
    pub clone_target_base_folder: String,
    pub clone_include_groups: bool,
    pub clone_include_files: bool,
    pub clone_include_unique_perms: bool,
    pub clone_overwrite: bool,
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
}

impl App {
    pub fn new(
        auth: AuthManager,
        db: Database,
        graph: GraphClient,
        spo: SpoClient,
    ) -> Self {
        let (tx_sender, rx_receiver) = mpsc::unbounded_channel();
        let cached_sites = db.load_cached_sites().unwrap_or_default();
        let sess = auth.load_session().ok().flatten();

        let (initial_screen, upn, disp, root) = if let Some(s) = sess {
            (
                Screen::SiteSelector,
                s.user_upn.unwrap_or_else(|| "usuario@m365".to_string()),
                s.user_display_name.unwrap_or_else(|| "Usuario M365".to_string()),
                s.root_site_url.unwrap_or_default(),
            )
        } else {
            (Screen::Auth, String::new(), String::new(), String::new())
        };

        let mut app = Self {
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
            sites: cached_sites,
            site_filter: String::new(),
            selected_site_idx: 0,
            current_site: None,
            groups: Vec::new(),
            selected_group_idx: 0,
            group_members: Vec::new(),
            selected_member_idx: 0,
            pane_filter: String::new(),
            libraries: Vec::new(),
            selected_library_idx: 0,
            folders: Vec::new(),
            selected_folder_idx: 0,
            clone_source_url: String::new(),
            clone_target_base_folder: String::new(),
            clone_include_groups: true,
            clone_include_files: true,
            clone_include_unique_perms: true,
            clone_overwrite: true,
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

    pub fn selected_folder(&self) -> Option<&SpoFolder> {
        self.folders.get(self.selected_folder_idx)
    }

    pub fn trigger_browser_login(&mut self) {
        self.busy_message = Some("Esperando inicio de sesión en el navegador (PKCE)...".to_string());
        let auth = self.auth.clone();
        let graph = self.graph.clone();
        let tx = self.tx_sender.clone();

        tokio::spawn(async move {
            match auth.login_interactive_browser(None, None).await {
                Ok(_) => match graph.get_me_and_root_site().await {
                    Ok((me, root)) => {
                        let _ = tx.send(AsyncEvent::AuthCompleted {
                            user_upn: me.user_principal_name.or(me.mail).unwrap_or_default(),
                            display_name: me.display_name.unwrap_or_default(),
                            root_url: root.web_url,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(AsyncEvent::Error(e.to_string()));
                    }
                },
                Err(e) => {
                    let _ = tx.send(AsyncEvent::Error(e.to_string()));
                }
            }
        });
    }

    pub fn trigger_device_code_login(&mut self) {
        self.busy_message = Some("Solicitando código de dispositivo a Entra ID...".to_string());
        let auth = self.auth.clone();
        let graph = self.graph.clone();
        let tx = self.tx_sender.clone();

        tokio::spawn(async move {
            match auth.start_device_code(None, None).await {
                Ok((prompt, tenant, client_id)) => {
                    let _ = tx.send(AsyncEvent::DeviceCodeReady(prompt.clone()));
                    match auth.poll_device_code(&prompt, &tenant, &client_id).await {
                        Ok(_) => match graph.get_me_and_root_site().await {
                            Ok((me, root)) => {
                                let _ = tx.send(AsyncEvent::AuthCompleted {
                                    user_upn: me
                                        .user_principal_name
                                        .or(me.mail)
                                        .unwrap_or_default(),
                                    display_name: me.display_name.unwrap_or_default(),
                                    root_url: root.web_url,
                                });
                            }
                            Err(e) => {
                                let _ = tx.send(AsyncEvent::Error(e.to_string()));
                            }
                        },
                        Err(e) => {
                            let _ = tx.send(AsyncEvent::Error(e.to_string()));
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(AsyncEvent::Error(e.to_string()));
                }
            }
        });
    }

    pub fn trigger_discover_sites(&mut self, keyword: Option<String>) {
        self.busy_message = Some(match &keyword {
            Some(k) => format!(
                "Buscando sitios SharePoint ('{}') y verificando rol Admin/Owner...",
                k
            ),
            None => "Descubriendo sitios SharePoint donde eres Administrador o Propietario...".to_string(),
        });
        let spo = self.spo.clone();
        let graph = self.graph.clone();
        let db = self.db.clone();
        let tx = self.tx_sender.clone();

        tokio::spawn(async move {
            match spo
                .discover_admin_and_owner_sites(&graph, keyword.as_deref())
                .await
            {
                Ok(sites) => {
                    let _ = db.save_cached_sites(&sites);
                    let _ = tx.send(AsyncEvent::SitesDiscovered(sites));
                }
                Err(e) => {
                    let _ = tx.send(AsyncEvent::Error(e.to_string()));
                }
            }
        });
    }

    pub fn trigger_check_manual_site(&mut self, url: String) {
        self.busy_message = Some(format!("Verificando acceso y permisos en '{}'...", url));
        let spo = self.spo.clone();
        let db = self.db.clone();
        let tx = self.tx_sender.clone();

        tokio::spawn(async move {
            match spo.check_user_site_role(&url).await {
                Ok(Some(site)) => {
                    let _ = db.save_cached_sites(std::slice::from_ref(&site));
                    let _ = tx.send(AsyncEvent::SingleSiteChecked(site));
                }
                Ok(None) => {
                    let _ = tx.send(AsyncEvent::Error(format!(
                        "No tienes permisos de Administrador ni Propietario en '{}'",
                        url
                    )));
                }
                Err(e) => {
                    let _ = tx.send(AsyncEvent::Error(e.to_string()));
                }
            }
        });
    }

    pub fn enter_site_workspace(&mut self, site: SpoSite) {
        let site_url = site.url.clone();
        self.current_site = Some(site);
        self.screen = Screen::SiteWorkspace;
        self.tab = WorkspaceTab::GroupsAndUsers;
        self.focus = PaneFocus::Left;
        self.groups.clear();
        self.group_members.clear();
        self.libraries.clear();
        self.folders.clear();
        self.selected_group_idx = 0;
        self.selected_member_idx = 0;
        self.selected_library_idx = 0;
        self.selected_folder_idx = 0;
        self.trigger_reload_workspace(&site_url);
    }

    pub fn trigger_reload_workspace(&mut self, site_url: &str) {
        self.busy_message = Some(format!("Cargando grupos, miembros y bibliotecas de '{}'...", site_url));
        let spo = self.spo.clone();
        let db = self.db.clone();
        let tx = self.tx_sender.clone();
        let url = site_url.to_string();

        tokio::spawn(async move {
            let groups_res = spo.get_site_groups(&url).await;
            let libs_res = spo.get_document_libraries(&url).await;
            match groups_res {
                Ok(groups) => {
                    let initial_members = if let Some(first_grp) = groups.first() {
                        spo.get_group_members(&url, first_grp.id)
                            .await
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    let libraries = libs_res.unwrap_or_default();
                    let initial_folders = if let Some(first_lib) = libraries.first() {
                        spo.list_folders(&url, &first_lib.title, None)
                            .await
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    let transactions = db.list_transactions(Some(&url), 50).unwrap_or_default();
                    let _ = tx.send(AsyncEvent::SiteWorkspaceLoaded {
                        groups,
                        initial_members,
                        libraries,
                        initial_folders,
                        transactions,
                    });
                }
                Err(e) => {
                    let _ = tx.send(AsyncEvent::Error(e.to_string()));
                }
            }
        });
    }

    pub fn trigger_load_group_members(&mut self, group_id: i64) {
        let Some(ref site) = self.current_site else {
            return;
        };
        let site_url = site.url.clone();
        self.busy_message = Some("Cargando miembros del grupo...".to_string());
        let spo = self.spo.clone();
        let tx = self.tx_sender.clone();

        tokio::spawn(async move {
            match spo.get_group_members(&site_url, group_id).await {
                Ok(members) => {
                    let _ = tx.send(AsyncEvent::GroupMembersLoaded { group_id, members });
                }
                Err(e) => {
                    let _ = tx.send(AsyncEvent::Error(e.to_string()));
                }
            }
        });
    }

    pub fn trigger_load_library_folders(&mut self, library_title: String) {
        let Some(ref site) = self.current_site else {
            return;
        };
        let site_url = site.url.clone();
        self.busy_message = Some(format!("Cargando carpetas de '{}'...", library_title));
        let spo = self.spo.clone();
        let tx = self.tx_sender.clone();

        tokio::spawn(async move {
            match spo.list_folders(&site_url, &library_title, None).await {
                Ok(folders) => {
                    let _ = tx.send(AsyncEvent::LibraryFoldersLoaded {
                        library_title,
                        folders,
                    });
                }
                Err(e) => {
                    let _ = tx.send(AsyncEvent::Error(e.to_string()));
                }
            }
        });
    }

    pub fn trigger_search_directory_users(&mut self, query: String) {
        let site_url = self.current_site.as_ref().map(|s| s.url.clone());
        let graph = self.graph.clone();
        let spo = self.spo.clone();
        let tx = self.tx_sender.clone();

        tokio::spawn(async move {
            let mut results = graph.search_directory_users(&query).await.unwrap_or_default();
            if results.is_empty() {
                if let Some(ref url) = site_url {
                    results = spo.search_people_picker(url, &query).await.unwrap_or_default();
                }
            }
            let _ = tx.send(AsyncEvent::DirectoryUsersFound(results));
        });
    }

    pub fn trigger_import_matrix_file(&mut self, file_path: String) {
        let Some(ref site) = self.current_site else {
            return;
        };
        let site_url = site.url.clone();
        let default_lib = self.selected_library().map(|l| l.title.clone());
        let tx = self.tx_sender.clone();

        tokio::spawn(async move {
            let path = PathBuf::from(file_path.trim());
            match MatrixImporter::load_from_file(&path, default_lib.as_deref()) {
                Ok(specs) => {
                    let q = MatrixImporter::specs_to_staging_queue(&site_url, &specs);
                    let src_name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("matrix")
                        .to_string();
                    let _ = tx.send(AsyncEvent::MatrixImported {
                        queue_ops: q.ops,
                        source_file: src_name,
                    });
                }
                Err(e) => {
                    let _ = tx.send(AsyncEvent::Error(e.to_string()));
                }
            }
        });
    }

    pub fn trigger_export_matrix_file(&mut self, file_path: String) {
        let Some(ref site) = self.current_site else {
            return;
        };
        let site_url = site.url.clone();
        let lib_title = self
            .selected_library()
            .map(|l| l.title.clone())
            .unwrap_or_else(|| "Documents".to_string());
        self.busy_message = Some(format!(
            "Exportando permisos únicos de '{}' a '{}'...",
            lib_title, file_path
        ));
        let spo = self.spo.clone();
        let tx = self.tx_sender.clone();

        tokio::spawn(async move {
            let path = PathBuf::from(file_path.trim());
            match MatrixImporter::export_library_permissions_to_file(&spo, &site_url, &lib_title, &path).await {
                Ok(count) => {
                    let _ = tx.send(AsyncEvent::MatrixExported {
                        count,
                        path: path.display().to_string(),
                    });
                }
                Err(e) => {
                    let _ = tx.send(AsyncEvent::Error(e.to_string()));
                }
            }
        });
    }

    pub fn trigger_plan_clone(&mut self) {
        let Some(ref target_site) = self.current_site else {
            return;
        };
        if self.clone_source_url.trim().is_empty() {
            self.status_toast = Some((
                "Indica primero la URL del sitio SharePoint Origen (pulsa 's')".to_string(),
                true,
            ));
            return;
        }
        let cfg = CloneConfig {
            source_site_url: self.clone_source_url.trim().to_string(),
            target_site_url: target_site.url.clone(),
            libraries_filter: None,
            target_base_folder: self.clone_target_base_folder.clone(),
            include_groups: self.clone_include_groups,
            include_files: self.clone_include_files,
            include_unique_permissions: self.clone_include_unique_perms,
            overwrite: self.clone_overwrite,
            note: Some(format!("Clonado desde {}", self.clone_source_url)),
        };

        self.busy_message = Some(format!(
            "Analizando sitio origen '{}' y preparando plan de clonado...",
            cfg.source_site_url
        ));
        let spo = self.spo.clone();
        let tx = self.tx_sender.clone();

        tokio::spawn(async move {
            let cloner = SiteClonerEngine::new(spo);
            match cloner.plan_clone(&cfg).await {
                Ok(q) => {
                    let _ = tx.send(AsyncEvent::ClonePlanned {
                        queue_ops: q.ops,
                        source_url: cfg.source_site_url,
                    });
                }
                Err(e) => {
                    let _ = tx.send(AsyncEvent::Error(e.to_string()));
                }
            }
        });
    }

    pub fn trigger_apply_staging(&mut self, note: Option<String>) {
        let Some(ref site) = self.current_site else {
            return;
        };
        if self.staging.is_empty() {
            self.status_toast = Some(("No hay cambios pendientes en Staging".to_string(), true));
            return;
        }

        let queue_clone = self.staging.clone();
        let actor = if self.user_upn.is_empty() {
            "tui-user".to_string()
        } else {
            self.user_upn.clone()
        };
        let site_url = site.url.clone();
        let site_title = site.title.clone();
        let source = self.last_staging_source.clone();

        self.busy_message = Some(format!(
            "Aplicando {} operación(es) en SharePoint y registrando transacción en SQLite...",
            queue_clone.len()
        ));

        let spo = self.spo.clone();
        let db = self.db.clone();
        let tx = self.tx_sender.clone();

        tokio::spawn(async move {
            let executor = TransactionExecutor::new(spo, db);
            match executor
                .apply_queue(
                    &queue_clone,
                    &actor,
                    &site_url,
                    &site_title,
                    &source,
                    note.as_deref(),
                    false,
                )
                .await
            {
                Ok(report) => {
                    let _ = tx.send(AsyncEvent::StagingApplied { report });
                }
                Err(e) => {
                    let _ = tx.send(AsyncEvent::Error(e.to_string()));
                }
            }
        });
    }

    pub fn trigger_revert_transaction(&mut self, tx_id: i64, only_op_id: Option<i64>) {
        self.busy_message = Some(format!(
            "Revirtiendo (Rollback) transacción #{} contra SharePoint Online...",
            tx_id
        ));
        let spo = self.spo.clone();
        let db = self.db.clone();
        let tx = self.tx_sender.clone();

        tokio::spawn(async move {
            let executor = TransactionExecutor::new(spo, db);
            match executor.revert_transaction(tx_id, only_op_id, false).await {
                Ok((updated_tx, ops)) => {
                    let reverted_cnt = ops.iter().filter(|o| o.status == "REVERTED").count();
                    let _ = tx.send(AsyncEvent::TransactionReverted {
                        tx: updated_tx,
                        message: format!(
                            "Rollback completado: {} operación(es) revertida(s) en transacción #{}",
                            reverted_cnt, tx_id
                        ),
                    });
                }
                Err(e) => {
                    let _ = tx.send(AsyncEvent::Error(e.to_string()));
                }
            }
        });
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
                self.status_toast = Some(("✓ Sesión iniciada en Microsoft 365".to_string(), false));
                self.trigger_discover_sites(None);
            }
            AsyncEvent::DeviceCodeReady(prompt) => {
                self.busy_message = Some(format!(
                    "Esperando confirmación del código {} en {}",
                    prompt.user_code, prompt.verification_uri
                ));
                self.device_code_prompt = Some(prompt);
            }
            AsyncEvent::SitesDiscovered(sites) => {
                self.busy_message = None;
                let count = sites.len();
                self.sites = sites;
                self.selected_site_idx = 0;
                self.status_toast = Some((
                    format!(
                        "✓ {} sitio(s) SharePoint cargados donde eres Admin o Propietario",
                        count
                    ),
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
                initial_folders,
                transactions,
            } => {
                self.busy_message = None;
                self.groups = groups;
                self.group_members = initial_members;
                self.libraries = libraries;
                self.folders = initial_folders;
                self.transactions = transactions;
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
                folders,
            } => {
                self.busy_message = None;
                if self.selected_library().map(|l| l.title.as_str()) == Some(library_title.as_str())
                {
                    self.folders = folders;
                    self.selected_folder_idx = 0;
                }
            }
            AsyncEvent::DirectoryUsersFound(results) => {
                if let Modal::AddUserToGroup {
                    results: ref mut modal_res,
                    selected_idx: ref mut sel,
                    searching: ref mut srch,
                    ..
                } = self.modal
                {
                    *modal_res = results;
                    *sel = 0;
                    *srch = false;
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
                    format!(
                        "✓ Matriz '{}' cargada: {} operación(es) añadidas a Staging. Pulsa 'A' para Aplicar.",
                        source_file, count
                    ),
                    false,
                ));
            }
            AsyncEvent::MatrixExported { count, path } => {
                self.busy_message = None;
                self.status_toast = Some((
                    format!(
                        "✓ Exportadas {} carpetas con permisos únicos en '{}'",
                        count, path
                    ),
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
                    format!(
                        "✓ Plan de clonado listo ({} operaciones en Staging). Revisa y pulsa 'A' para Aplicar.",
                        count
                    ),
                    false,
                ));
            }
            AsyncEvent::StagingApplied { report } => {
                self.busy_message = None;
                self.staging.clear();
                self.last_staging_source = "TUI".to_string();
                let is_err = report.status == "FAILED";
                self.status_toast = Some((
                    format!(
                        "✓ Transacción #{:?} [{}]: {}",
                        report.transaction_id.unwrap_or(0),
                        report.status,
                        report.summary
                    ),
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
