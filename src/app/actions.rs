use crate::cloner::SiteClonerEngine;
use crate::i18n::Lang;
use crate::importer::MatrixImporter;
use crate::models::{
    CloneConfig, CloneMode, SpoSite,
};
use crate::staging::TransactionExecutor;
use std::path::PathBuf;


use super::*;

impl App {
    pub fn trigger_browser_login(&mut self) {
        let Some(target) = self.resolve_login_target_or_toast() else {
            return;
        };
        self.busy_message = Some(
            self.lang
                .tr(
                    "Resolviendo tenant e iniciando sesión en el navegador (PKCE)...",
                    "Resolving tenant and signing in via browser (PKCE)...",
                )
                .to_string(),
        );
        let auth = self.auth.clone();
        let graph = self.graph.clone();
        let tx = self.tx_sender.clone();

        tokio::spawn(async move {
            match auth.login_interactive_browser(Some(&target), None).await {
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
        let Some(target) = self.resolve_login_target_or_toast() else {
            return;
        };
        self.busy_message = Some(
            self.lang
                .tr(
                    "Resolviendo tenant y solicitando código de dispositivo a Entra ID...",
                    "Resolving tenant and requesting device code from Entra ID...",
                )
                .to_string(),
        );
        let auth = self.auth.clone();
        let graph = self.graph.clone();
        let tx = self.tx_sender.clone();

        tokio::spawn(async move {
            match auth.start_device_code(Some(&target), None).await {
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
        let lang = self.lang;
        self.busy_message = Some(match &keyword {
            Some(k) => match lang {
                Lang::Es => format!(
                    "Buscando sitios SharePoint ('{}') y verificando rol Admin/Owner...",
                    k
                ),
                Lang::En => format!(
                    "Searching SharePoint sites ('{}') and checking Admin/Owner role...",
                    k
                ),
            },
            None => lang
                .tr(
                    "Descubriendo sitios SharePoint donde eres Administrador o Propietario...",
                    "Discovering SharePoint sites where you are Admin or Owner...",
                )
                .to_string(),
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
        let lang = self.lang;
        self.busy_message = Some(match lang {
            Lang::Es => format!("Verificando acceso y permisos en '{}'...", url),
            Lang::En => format!("Checking access and permissions on '{}'...", url),
        });
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
                    let _ = tx.send(AsyncEvent::Error(match lang {
                        Lang::Es => format!(
                            "No tienes permisos de Administrador ni Propietario en '{}'",
                            url
                        ),
                        Lang::En => format!(
                            "You do not have Admin or Owner permissions on '{}'",
                            url
                        ),
                    }));
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
        self.site_exclude_from_offline_client = None;
        self.screen = Screen::SiteWorkspace;
        self.tab = WorkspaceTab::GroupsAndUsers;
        self.focus = PaneFocus::Left;
        self.groups.clear();
        self.group_members.clear();
        self.libraries.clear();
        self.current_folder_path.clear();
        self.folder_ancestors.clear();
        self.folders.clear();
        self.selected_group_idx = 0;
        self.selected_member_idx = 0;
        self.selected_library_idx = 0;
        self.selected_folder_idx = 0;
        self.trigger_reload_workspace(&site_url);
    }

    pub fn trigger_reload_workspace(&mut self, site_url: &str) {
        self.busy_message = Some(match self.lang {
            Lang::Es => format!("Cargando grupos, miembros y bibliotecas de '{}'...", site_url),
            Lang::En => format!("Loading groups, members, and libraries from '{}'...", site_url),
        });
        let spo = self.spo.clone();
        let db = self.db.clone();
        let tx = self.tx_sender.clone();
        let url = site_url.to_string();
        let preferred_lib_title = self.selected_library().map(|l| l.title.clone());
        let preferred_folder_path = self.current_folder_path.clone();

        tokio::spawn(async move {
            let (groups_res, libs_res, web_sync_res) = tokio::join!(
                spo.get_site_groups(&url),
                spo.get_document_libraries(&url),
                spo.get_web_offline_sync(&url)
            );
            match groups_res {
                Ok(groups) => {
                    let initial_members = if let Some(first_grp) = groups.first() {
                        spo.get_group_members(&url, first_grp.id)
                            .await
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    let libraries = spo
                        .populate_libraries_shortcut_status(&url, libs_res.unwrap_or_default())
                        .await;
                    let target_lib_title = preferred_lib_title
                        .filter(|t| libraries.iter().any(|l| l.title.eq_ignore_ascii_case(t)))
                        .or_else(|| libraries.first().map(|l| l.title.clone()));

                    let (loaded_folder_path, initial_ancestors, initial_folders) =
                        if let Some(ref lib_title) = target_lib_title {
                            let sub = if preferred_folder_path.is_empty() {
                                None
                            } else {
                                Some(preferred_folder_path.as_str())
                            };
                            match spo.list_folders_with_context(&url, lib_title, sub).await {
                                Ok((anc, flds)) => (preferred_folder_path, anc, flds),
                                Err(_) if sub.is_some() => {
                                    let (anc, flds) = spo
                                        .list_folders_with_context(&url, lib_title, None)
                                        .await
                                        .unwrap_or_default();
                                    (String::new(), anc, flds)
                                }
                                Err(_) => (String::new(), Vec::new(), Vec::new()),
                            }
                        } else {
                            (String::new(), Vec::new(), Vec::new())
                        };
                    let transactions = db.list_transactions(Some(&url), 50).unwrap_or_default();
                    let site_exclude_from_offline_client = web_sync_res.ok().map(|(_, ex)| ex);
                    let _ = tx.send(AsyncEvent::SiteWorkspaceLoaded {
                        groups,
                        initial_members,
                        libraries,
                        loaded_library_title: target_lib_title,
                        loaded_folder_path,
                        initial_ancestors,
                        initial_folders,
                        transactions,
                        site_exclude_from_offline_client,
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
        self.busy_message = Some(
            self.lang
                .tr(
                    "Cargando miembros del grupo...",
                    "Loading group members...",
                )
                .to_string(),
        );
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

    pub fn trigger_load_library_folders(
        &mut self,
        library_title: String,
        relative_subpath: Option<String>,
        select_folder_name: Option<String>,
    ) {
        let Some(ref site) = self.current_site else {
            return;
        };
        let clean_sub = relative_subpath
            .as_deref()
            .unwrap_or("")
            .trim_matches('/')
            .to_string();
        self.current_folder_path = clean_sub.clone();
        let site_url = site.url.clone();
        self.busy_message = Some(match (clean_sub.is_empty(), self.lang) {
            (true, Lang::Es) => format!("Cargando carpetas de '{}'...", library_title),
            (true, Lang::En) => format!("Loading folders from '{}'...", library_title),
            (false, Lang::Es) => format!("Cargando carpetas de '{}/{}'...", library_title, clean_sub),
            (false, Lang::En) => format!("Loading folders from '{}/{}'...", library_title, clean_sub),
        });
        let spo = self.spo.clone();
        let tx = self.tx_sender.clone();

        tokio::spawn(async move {
            let sub_opt = if clean_sub.is_empty() {
                None
            } else {
                Some(clean_sub.as_str())
            };
            match spo
                .list_folders_with_context(&site_url, &library_title, sub_opt)
                .await
            {
                Ok((ancestors, folders)) => {
                    let _ = tx.send(AsyncEvent::LibraryFoldersLoaded {
                        library_title,
                        relative_subpath: clean_sub,
                        ancestors,
                        folders,
                        select_folder_name,
                    });
                }
                Err(e) => {
                    let _ = tx.send(AsyncEvent::Error(e.to_string()));
                }
            }
        });
    }

    pub fn navigate_into_selected_folder(&mut self) {
        if self.is_parent_row_selected() {
            self.navigate_up_folder();
            return;
        }
        let Some(lib_title) = self.selected_library().map(|l| l.title.clone()) else {
            return;
        };
        let Some(folder) = self.selected_folder().cloned() else {
            return;
        };
        self.trigger_load_library_folders(lib_title, Some(folder.relative_path), None);
    }

    pub fn navigate_up_folder(&mut self) -> bool {
        if self.current_folder_path.is_empty() {
            return false;
        }
        let Some(lib_title) = self.selected_library().map(|l| l.title.clone()) else {
            return false;
        };
        let clean = self.current_folder_path.trim_matches('/').to_string();
        let (parent_opt, child_name) = match clean.rsplit_once('/') {
            Some((parent, child)) => (Some(parent.to_string()), child.to_string()),
            None => (None, clean),
        };
        self.trigger_load_library_folders(lib_title, parent_opt, Some(child_name));
        true
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
            let _ = tx.send(AsyncEvent::DirectoryUsersFound { query, results });
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
        self.busy_message = Some(match self.lang {
            Lang::Es => format!(
                "Exportando permisos únicos de '{}' a '{}'...",
                lib_title, file_path
            ),
            Lang::En => format!(
                "Exporting unique permissions from '{}' to '{}'...",
                lib_title, file_path
            ),
        });
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
                self.lang
                    .tr(
                        "Indica primero la URL del sitio SharePoint Origen (pulsa 's')",
                        "Enter the Source SharePoint site URL first (press 's')",
                    )
                    .to_string(),
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
            note: Some(format!(
                "Clonado {} desde {}",
                if self.clone_destructive {
                    "DESTRUCTIVO"
                } else {
                    "aditivo"
                },
                self.clone_source_url
            )),
            mode: if self.clone_destructive {
                CloneMode::Destructive
            } else {
                CloneMode::Additive
            },
            current_actor_upn: if self.user_upn.is_empty() {
                None
            } else {
                Some(self.user_upn.clone())
            },
        };

        self.busy_message = Some(match self.lang {
            Lang::Es => format!(
                "Analizando sitio origen '{}' y preparando plan de clonado...",
                cfg.source_site_url
            ),
            Lang::En => format!(
                "Analyzing source site '{}' and building clone plan...",
                cfg.source_site_url
            ),
        });
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
            self.status_toast = Some((
                self.lang
                    .tr(
                        "No hay cambios pendientes en Staging",
                        "No pending changes in Staging",
                    )
                    .to_string(),
                true,
            ));
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

        self.busy_message = Some(match self.lang {
            Lang::Es => format!(
                "Aplicando {} operación(es) en SharePoint y registrando transacción en SQLite...",
                queue_clone.len()
            ),
            Lang::En => format!(
                "Applying {} operation(s) in SharePoint and recording transaction in SQLite...",
                queue_clone.len()
            ),
        });

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
        let lang = self.lang;
        self.busy_message = Some(match lang {
            Lang::Es => format!(
                "Revirtiendo (Rollback) transacción #{} contra SharePoint Online...",
                tx_id
            ),
            Lang::En => format!(
                "Reverting (Rollback) transaction #{} against SharePoint Online...",
                tx_id
            ),
        });
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
                        message: match lang {
                            Lang::Es => format!(
                                "Rollback completado: {} operación(es) revertida(s) en transacción #{}",
                                reverted_cnt, tx_id
                            ),
                            Lang::En => format!(
                                "Rollback complete: {} operation(s) reverted in transaction #{}",
                                reverted_cnt, tx_id
                            ),
                        },
                    });
                }
                Err(e) => {
                    let _ = tx.send(AsyncEvent::Error(e.to_string()));
                }
            }
        });
    }

}
