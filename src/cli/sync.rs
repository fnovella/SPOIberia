use crate::api::spo::SpoClient;
use crate::models::{
    SiteSyncStatus, StagingOp,
};
use crate::staging::{ExecutionReport, StagingQueue, TransactionExecutor};
use anyhow::Result;


use super::*;

pub(super) fn enqueue_sync_from_status(
    node: &SiteSyncStatus,
    target_exclude: bool,
    site_only: bool,
    libraries_only: bool,
    target: SyncTarget,
    q: &mut StagingQueue,
) {
    let manage_sharepoint = matches!(target, SyncTarget::All | SyncTarget::Sharepoint);
    let manage_onedrive = matches!(target, SyncTarget::All | SyncTarget::Onedrive);

    if manage_sharepoint && !libraries_only && node.exclude_from_offline_client != target_exclude {
        q.push(StagingOp::SetSiteOfflineSync {
            site_url: node.site_url.clone(),
            site_title: node.site_title.clone(),
            exclude_from_offline_client: target_exclude,
        });
    }
    if !site_only {
        for lib in &node.libraries {
            if manage_sharepoint {
                let sync_views_differ = if target_exclude {
                    lib.views_total > 0 && lib.views_hiding_sync < lib.views_total
                } else {
                    lib.views_hiding_sync > 0
                };
                if lib.exclude_from_offline_client != target_exclude || sync_views_differ {
                    q.push(StagingOp::SetLibraryOfflineSync {
                        site_url: node.site_url.clone(),
                        library_id: lib.id.clone(),
                        library_title: lib.title.clone(),
                        exclude_from_offline_client: target_exclude,
                        apply_view_formatting: false,
                    });
                }
            }
            if manage_onedrive {
                let views_differ = if target_exclude {
                    lib.views_total > 0 && !lib.shortcut_button_hidden
                } else {
                    lib.views_hiding_shortcut > 0
                };
                if views_differ {
                    q.push(StagingOp::SetLibraryOneDriveShortcuts {
                        site_url: node.site_url.clone(),
                        library_id: lib.id.clone(),
                        library_title: lib.title.clone(),
                        hide_shortcut_button: target_exclude,
                    });
                }
            }
        }
    }
    for sub in &node.subsites {
        enqueue_sync_from_status(
            sub,
            target_exclude,
            site_only,
            libraries_only,
            target,
            q,
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_sync_toggle(
    spo: &SpoClient,
    executor: &TransactionExecutor,
    actor_upn: &str,
    site: &str,
    library: &Option<String>,
    site_only: bool,
    libraries_only: bool,
    include_subsites: bool,
    target: SyncTarget,
    note: &Option<String>,
    dry_run: bool,
    target_exclude: bool,
    format: OutputFormat,
) -> Result<()> {
    if target == SyncTarget::Onedrive && site_only {
        anyhow::bail!(
            "--site-only no aplica a los accesos directos de OneDrive, que se gestionan a nivel de vistas de biblioteca"
        );
    }

    let manage_sharepoint = matches!(target, SyncTarget::All | SyncTarget::Sharepoint);
    let manage_onedrive = matches!(target, SyncTarget::All | SyncTarget::Onedrive);

    let mut q = StagingQueue::new();
    let mut site_title = site.to_string();

    if let Some(raw_libs) = library {
        let mut seen_ids = std::collections::HashSet::new();
        for name in raw_libs.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let lib = spo.resolve_library(site, name).await?;
            if seen_ids.insert(lib.id.to_lowercase()) {
                let clean_site = site.trim_end_matches('/').to_string();
                let views = spo
                    .get_library_views(site, &lib.id)
                    .await
                    .unwrap_or_default();
                if manage_sharepoint {
                    let sync_views_differ = views.iter().any(|v| {
                        SpoClient::build_sync_button_view_formatter(
                            v.custom_formatter.as_deref(),
                            target_exclude,
                        )
                        .is_some()
                    });
                    if lib.exclude_from_offline_client != target_exclude || sync_views_differ {
                        q.push(StagingOp::SetLibraryOfflineSync {
                            site_url: clean_site.clone(),
                            library_id: lib.id.clone(),
                            library_title: lib.title.clone(),
                            exclude_from_offline_client: target_exclude,
                            apply_view_formatting: false,
                        });
                    }
                }
                if manage_onedrive {
                    let views_differ = views.iter().any(|v| {
                        SpoClient::build_shortcut_view_formatter(
                            v.custom_formatter.as_deref(),
                            target_exclude,
                        )
                        .is_some()
                    });
                    if views_differ {
                        q.push(StagingOp::SetLibraryOneDriveShortcuts {
                            site_url: clean_site,
                            library_id: lib.id,
                            library_title: lib.title,
                            hide_shortcut_button: target_exclude,
                        });
                    }
                }
            }
        }
        if seen_ids.is_empty() {
            anyhow::bail!("Debes indicar al menos un nombre de biblioteca válido en --library");
        }
    } else {
        let status = spo.get_site_sync_status(site, include_subsites).await?;
        site_title = status.site_title.clone();
        enqueue_sync_from_status(
            &status,
            target_exclude,
            site_only,
            libraries_only,
            target,
            &mut q,
        );
    }

    if q.is_empty() {
        let desired = match (target, target_exclude) {
            (SyncTarget::Sharepoint, true) => {
                "sincronización SharePoint bloqueada (ExcludeFromOfflineClient=true)"
            }
            (SyncTarget::Sharepoint, false) => {
                "sincronización SharePoint permitida (ExcludeFromOfflineClient=false)"
            }
            (SyncTarget::Onedrive, true) => "accesos directos a OneDrive ocultos",
            (SyncTarget::Onedrive, false) => "accesos directos a OneDrive visibles",
            (SyncTarget::All, true) => {
                "sincronización SharePoint bloqueada y accesos directos a OneDrive ocultos"
            }
            (SyncTarget::All, false) => {
                "sincronización SharePoint permitida y accesos directos a OneDrive visibles"
            }
        };
        let report = ExecutionReport {
            status: "NO_CHANGES".to_string(),
            dry_run,
            transaction_id: None,
            site_url: site.to_string(),
            summary: format!("Sin cambios: el objetivo ya tenía {}", desired),
            operations: Vec::new(),
        };
        return print_execution_report(&report, format);
    }

    let report = executor
        .apply_queue(
            &q,
            actor_upn,
            site,
            &site_title,
            "CLI",
            note.as_deref(),
            dry_run,
        )
        .await?;
    print_execution_report(&report, format)
}

pub(super) fn print_site_sync_status_table(status: &SiteSyncStatus, depth: usize) {
    let indent = "  ".repeat(depth);
    let web_state = if status.exclude_from_offline_client {
        "BLOQUEADA (ExcludeFromOfflineClient=true)"
    } else {
        "ACTIVA    (ExcludeFromOfflineClient=false)"
    };
    println!(
        "{}Sitio: {} ({})",
        indent, status.site_title, status.site_url
    );
    println!("{}  Estado Sitio (SP.Web): {}", indent, web_state);
    if status.libraries.is_empty() {
        println!("{}  (Sin bibliotecas de documentos)", indent);
    } else {
        println!(
            "{}  {:<28} | {:<10} | {:<16} | RUTA RELATIVA",
            indent, "BIBLIOTECA", "SYNC", "BOTÓN ONEDRIVE"
        );
        println!("{}  {}", indent, "-".repeat(94));
        for lib in &status.libraries {
            let lib_state = if lib.exclude_from_offline_client {
                "Bloqueada"
            } else {
                "Activa"
            };
            let btn_state = if lib.views_total == 0 {
                "-".to_string()
            } else if lib.shortcut_button_hidden {
                format!("Oculto ({}/{})", lib.views_hiding_shortcut, lib.views_total)
            } else if lib.views_hiding_shortcut > 0 {
                format!("Parcial ({}/{})", lib.views_hiding_shortcut, lib.views_total)
            } else {
                format!("Visible (0/{})", lib.views_total)
            };
            println!(
                "{}  {:<28} | {:<10} | {:<16} | {}",
                indent,
                truncate(&lib.title, 28),
                lib_state,
                btn_state,
                lib.server_relative_url
            );
        }
    }
    for sub in &status.subsites {
        println!();
        print_site_sync_status_table(sub, depth + 1);
    }
}

