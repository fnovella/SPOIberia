pub mod api;
pub mod app;
pub mod auth;
pub mod cli;
pub mod cloner;
pub mod db;
pub mod importer;
pub mod models;
pub mod staging;
pub mod ui;

use crate::api::graph::GraphClient;
use crate::api::spo::SpoClient;
use crate::app::{
    App, Modal, PaneFocus, Screen, TextInputKind, WorkspaceTab, STANDARD_ROLES,
};
use crate::auth::AuthManager;
use crate::cli::{Cli, Commands};
use crate::db::Database;
use crate::models::StagingOp;
use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::collections::HashSet;
use std::io;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        None | Some(Commands::Tui) => run_tui().await,
        Some(cmd) => {
            if let Err(err) = cli::run_cli_command(&cli, cmd).await {
                if cli.effective_format() == cli::OutputFormat::Json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "status": "ERROR",
                            "message": err.to_string()
                        }))?
                    );
                } else {
                    eprintln!("Error: {}", err);
                }
                std::process::exit(1);
            }
            Ok(())
        }
    }
}

async fn run_tui() -> Result<()> {
    let auth = AuthManager::new()?;
    let db = Database::open_default()?;
    let graph = GraphClient::new(auth.clone())?;
    let spo = SpoClient::new(auth.clone())?;

    let mut app = App::new(auth, db, graph, spo);

    // Install panic hook so terminal is always restored cleanly
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(panic_info);
    }));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = tui_event_loop(&mut terminal, &mut app).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    res
}

async fn tui_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|f| ui::draw_ui(f, app))?;

        while let Ok(ev) = app.rx_receiver.try_recv() {
            app.handle_async_event(ev);
        }

        if app.should_quit {
            break;
        }

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    handle_key_event(app, key);
                }
            }
        }
    }
    Ok(())
}

fn handle_key_event(app: &mut App, key: KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return;
    }

    // 1. Handle active modal first
    if !matches!(app.modal, Modal::None) {
        handle_modal_key(app, key);
        return;
    }

    if key.code == KeyCode::Char('?') {
        app.modal = Modal::Help;
        return;
    }

    match app.screen {
        Screen::Auth => match key.code {
            KeyCode::Esc => app.should_quit = true,
            KeyCode::Enter => app.trigger_browser_login(),
            KeyCode::F(2) => app.trigger_device_code_login(),
            KeyCode::Char('d') | KeyCode::Char('D')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                app.trigger_device_code_login();
            }
            KeyCode::Backspace => {
                app.login_email_input.pop();
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                app.login_email_input.push(c);
            }
            _ => {}
        },
        Screen::SiteSelector => handle_site_selector_keys(app, key),
        Screen::SiteWorkspace => handle_workspace_keys(app, key),
    }
}

fn move_list_index(current: usize, len: usize, code: KeyCode) -> usize {
    if len == 0 {
        return 0;
    }
    match code {
        KeyCode::Down | KeyCode::Char('j') => (current + 1) % len,
        KeyCode::Up | KeyCode::Char('k') => current.checked_sub(1).unwrap_or(len - 1),
        KeyCode::PageDown => (current + 10).min(len - 1),
        KeyCode::PageUp => current.saturating_sub(10),
        KeyCode::Home => 0,
        KeyCode::End => len - 1,
        _ => current,
    }
}

fn handle_site_selector_keys(app: &mut App, key: KeyEvent) {
    let total = app.filtered_sites().len();
    match key.code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Down
        | KeyCode::Char('j')
        | KeyCode::Up
        | KeyCode::Char('k')
        | KeyCode::PageDown
        | KeyCode::PageUp
        | KeyCode::Home
        | KeyCode::End => {
            if total > 0 {
                app.selected_site_idx = move_list_index(app.selected_site_idx, total, key.code);
            }
        }
        KeyCode::Enter => {
            if let Some(site) = app.selected_site() {
                app.enter_site_workspace(site);
            }
        }
        KeyCode::Char('/') => {
            app.modal = Modal::TextInput {
                title: "Filtrar Sitios Localmente".to_string(),
                prompt: "Escribe parte del título, URL o rol (deja vacío para mostrar todos):".to_string(),
                value: app.site_filter.clone(),
                kind: TextInputKind::FilterSitesLocal,
            };
        }
        KeyCode::Char('s') => {
            app.modal = Modal::TextInput {
                title: "Buscar Sitios en el Tenant M365".to_string(),
                prompt: "Palabra clave a buscar en SharePoint Search (verifica rol Admin/Owner):".to_string(),
                value: String::new(),
                kind: TextInputKind::SearchSitesTenant,
            };
        }
        KeyCode::Char('u') => {
            app.modal = Modal::TextInput {
                title: "Abrir Sitio SharePoint por URL".to_string(),
                prompt: "Introduce la URL completa del sitio SharePoint:".to_string(),
                value: "https://".to_string(),
                kind: TextInputKind::ManualSiteUrl,
            };
        }
        KeyCode::Char('c') => {
            if let Some(site) = app.selected_site() {
                app.clone_source_url = site.url.clone();
                app.status_toast = Some((
                    format!(
                        "✓ Sitio '{}' fijado como Origen para Clonado (Fase 3)",
                        site.title
                    ),
                    false,
                ));
            }
        }
        KeyCode::Char('r') => {
            app.trigger_discover_sites(None);
        }
        _ => {}
    }
}

fn handle_workspace_keys(app: &mut App, key: KeyEvent) {
    // Global Workspace navigation keys
    match key.code {
        KeyCode::Esc | KeyCode::Backspace => {
            app.screen = Screen::SiteSelector;
            return;
        }
        KeyCode::Char('q') => {
            app.should_quit = true;
            return;
        }
        KeyCode::Char('1') | KeyCode::F(1) => {
            app.tab = WorkspaceTab::GroupsAndUsers;
            app.focus = PaneFocus::Left;
            return;
        }
        KeyCode::Char('2') | KeyCode::F(2) => {
            app.tab = WorkspaceTab::LibrariesAndFolders;
            app.focus = PaneFocus::Left;
            return;
        }
        KeyCode::Char('3') | KeyCode::F(3) => {
            app.tab = WorkspaceTab::CloneSite;
            return;
        }
        KeyCode::Char('4') | KeyCode::F(4) | KeyCode::Char('H') | KeyCode::Char('C') => {
            app.tab = WorkspaceTab::StagingAndHistory;
            app.refresh_transactions_from_db();
            return;
        }
        KeyCode::Char('A') => {
            if app.staging.is_empty() {
                app.status_toast = Some((
                    "No hay cambios pendientes en la cola de Staging para aplicar".to_string(),
                    true,
                ));
            } else {
                app.modal = Modal::ConfirmApplyStaging {
                    note: String::new(),
                };
            }
            return;
        }
        KeyCode::Char('O') => {
            app.toggle_site_and_libraries_offline_sync();
            return;
        }
        KeyCode::Tab | KeyCode::Left | KeyCode::Right | KeyCode::Char('h') | KeyCode::Char('l') => {
            app.focus = match app.focus {
                PaneFocus::Left => PaneFocus::Right,
                PaneFocus::Right => PaneFocus::Left,
            };
            return;
        }
        _ => {}
    }

    match app.tab {
        WorkspaceTab::GroupsAndUsers => handle_tab_groups_keys(app, key),
        WorkspaceTab::LibrariesAndFolders => handle_tab_folders_keys(app, key),
        WorkspaceTab::CloneSite => handle_tab_clone_keys(app, key),
        WorkspaceTab::StagingAndHistory => handle_tab_staging_history_keys(app, key),
    }
}

fn handle_tab_groups_keys(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Down
        | KeyCode::Char('j')
        | KeyCode::Up
        | KeyCode::Char('k')
        | KeyCode::PageDown
        | KeyCode::PageUp
        | KeyCode::Home
        | KeyCode::End => match app.focus {
            PaneFocus::Left => {
                if !app.groups.is_empty() {
                    let next = move_list_index(app.selected_group_idx, app.groups.len(), key.code);
                    if next != app.selected_group_idx {
                        app.selected_group_idx = next;
                        if let Some(grp_id) = app.selected_group().map(|g| g.id) {
                            app.trigger_load_group_members(grp_id);
                        }
                    }
                }
            }
            PaneFocus::Right => {
                if !app.group_members.is_empty() {
                    app.selected_member_idx =
                        move_list_index(app.selected_member_idx, app.group_members.len(), key.code);
                }
            }
        },
        KeyCode::Char('a') => {
            if let Some(grp) = app.selected_group() {
                app.modal = Modal::AddUserToGroup {
                    group_id: grp.id,
                    group_title: grp.title.clone(),
                    query: String::new(),
                    results: Vec::new(),
                    selected_idx: 0,
                    marked_identifiers: HashSet::new(),
                    searching: false,
                };
            }
        }
        KeyCode::Char('d') | KeyCode::Delete | KeyCode::Char('x') => {
            let Some(ref site) = app.current_site else {
                return;
            };
            let Some(grp) = app.selected_group().cloned() else {
                return;
            };
            if let Some(member) = app.group_members.get(app.selected_member_idx).cloned() {
                let ident = member.clean_identifier();
                app.staging.push(StagingOp::RemoveUserFromGroup {
                    site_url: site.url.clone(),
                    group_id: grp.id,
                    group_title: grp.title.clone(),
                    user_id: Some(member.id),
                    user_identifier: ident.clone(),
                    user_display_name: member.title.clone(),
                });
                app.status_toast = Some((
                    format!(
                        "Encolada baja de '{}' en '{}'. Pulsa 'A' para Aplicar.",
                        member.title, grp.title
                    ),
                    false,
                ));
            }
        }
        KeyCode::Char('g') => {
            app.modal = Modal::TextInput {
                title: "Crear Nuevo Grupo de SharePoint".to_string(),
                prompt: "Nombre del nuevo grupo de SharePoint a crear en el sitio:".to_string(),
                value: String::new(),
                kind: TextInputKind::CreateSharePointGroup,
            };
        }
        KeyCode::Char('r') => {
            if let Some(site_url) = app.current_site.as_ref().map(|s| s.url.clone()) {
                app.trigger_reload_workspace(&site_url);
            }
        }
        _ => {}
    }
}

fn handle_tab_folders_keys(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Down
        | KeyCode::Char('j')
        | KeyCode::Up
        | KeyCode::Char('k')
        | KeyCode::PageDown
        | KeyCode::PageUp
        | KeyCode::Home
        | KeyCode::End => match app.focus {
            PaneFocus::Left => {
                if !app.libraries.is_empty() {
                    let next =
                        move_list_index(app.selected_library_idx, app.libraries.len(), key.code);
                    if next != app.selected_library_idx {
                        app.selected_library_idx = next;
                        if let Some(lib_title) = app.selected_library().map(|l| l.title.clone()) {
                            app.trigger_load_library_folders(lib_title);
                        }
                    }
                }
            }
            PaneFocus::Right => {
                if !app.folders.is_empty() {
                    app.selected_folder_idx =
                        move_list_index(app.selected_folder_idx, app.folders.len(), key.code);
                }
            }
        },
        KeyCode::Char('n') => {
            app.modal = Modal::TextInput {
                title: "Crear Nueva Carpeta (en Biblioteca Actual)".to_string(),
                prompt: "Ruta relativa de la carpeta a crear (ej. 01_Direccion/Confidencial):".to_string(),
                value: String::new(),
                kind: TextInputKind::CreateFolderRelativePath,
            };
        }
        KeyCode::Char('b') => {
            app.modal = Modal::TextInput {
                title: "Crear Nueva Biblioteca de Documentos".to_string(),
                prompt: "Título de la nueva Biblioteca de Documentos:".to_string(),
                value: String::new(),
                kind: TextInputKind::CreateDocumentLibrary,
            };
        }
        KeyCode::Char('o') => {
            app.toggle_selected_library_offline_sync();
        }
        KeyCode::Char('p') => {
            let lib_title = app
                .selected_library()
                .map(|l| l.title.clone())
                .unwrap_or_else(|| "Documents".to_string());
            let rel_path = app
                .selected_folder()
                .map(|f| f.relative_path.clone())
                .unwrap_or_default();
            if rel_path.is_empty() {
                app.status_toast = Some((
                    "Selecciona o crea primero una carpeta para asignarle permisos".to_string(),
                    true,
                ));
                return;
            }
            app.modal = Modal::GrantFolderPermission {
                library_title: lib_title,
                relative_folder_path: rel_path,
                group_name: String::new(),
                role_idx: 2, // Default "Edit"
                copy_parent_roles: false,
                clear_subscopes: false,
                create_group_if_missing: true,
                active_field: 0,
            };
        }
        KeyCode::Char('i') => {
            app.modal = Modal::TextInput {
                title: "Importar Matriz de Carpetas y Permisos (CSV / JSON)".to_string(),
                prompt: "Ruta del archivo externo (.csv o .json):".to_string(),
                value: "./examples/sample-permissions.csv".to_string(),
                kind: TextInputKind::ImportMatrixFilePath,
            };
        }
        KeyCode::Char('e') => {
            app.modal = Modal::TextInput {
                title: "Exportar Matriz de Permisos Únicos (CSV / JSON)".to_string(),
                prompt: "Ruta del archivo de salida (.csv o .json):".to_string(),
                value: "./exported-permissions.json".to_string(),
                kind: TextInputKind::ExportMatrixFilePath,
            };
        }
        KeyCode::Char('r') => {
            if let Some(lib_title) = app.selected_library().map(|l| l.title.clone()) {
                app.trigger_load_library_folders(lib_title);
            }
        }
        _ => {}
    }
}

fn handle_tab_clone_keys(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => {
            app.clone_selected_option = (app.clone_selected_option + 1) % 4;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.clone_selected_option = app.clone_selected_option.checked_sub(1).unwrap_or(3);
        }
        KeyCode::Char(' ') => match app.clone_selected_option {
            0 => app.clone_include_groups = !app.clone_include_groups,
            1 => app.clone_include_files = !app.clone_include_files,
            2 => app.clone_include_unique_perms = !app.clone_include_unique_perms,
            3 => app.clone_overwrite = !app.clone_overwrite,
            _ => {}
        },
        KeyCode::Char('s') => {
            app.modal = Modal::TextInput {
                title: "Sitio SharePoint Origen para Clonado".to_string(),
                prompt: "URL del sitio SharePoint Origen desde el que copiar grupos, datos y permisos:".to_string(),
                value: app.clone_source_url.clone(),
                kind: TextInputKind::SetCloneSourceUrl,
            };
        }
        KeyCode::Enter => {
            app.trigger_plan_clone();
        }
        _ => {}
    }
}

fn handle_tab_staging_history_keys(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => match app.focus {
            PaneFocus::Left => {
                if !app.staging.is_empty() {
                    app.selected_staging_idx =
                        (app.selected_staging_idx + 1) % app.staging.len();
                }
            }
            PaneFocus::Right => {
                if !app.transactions.is_empty() {
                    app.selected_tx_idx = (app.selected_tx_idx + 1) % app.transactions.len();
                    if let Some(tx) = app.transactions.get(app.selected_tx_idx) {
                        app.selected_tx_ops = app
                            .db
                            .get_transaction_operations(tx.id)
                            .unwrap_or_default();
                        app.selected_tx_op_idx = 0;
                    }
                }
            }
        },
        KeyCode::Up | KeyCode::Char('k') => match app.focus {
            PaneFocus::Left => {
                if !app.staging.is_empty() {
                    app.selected_staging_idx = app
                        .selected_staging_idx
                        .checked_sub(1)
                        .unwrap_or(app.staging.len() - 1);
                }
            }
            PaneFocus::Right => {
                if !app.transactions.is_empty() {
                    app.selected_tx_idx = app
                        .selected_tx_idx
                        .checked_sub(1)
                        .unwrap_or(app.transactions.len() - 1);
                    if let Some(tx) = app.transactions.get(app.selected_tx_idx) {
                        app.selected_tx_ops = app
                            .db
                            .get_transaction_operations(tx.id)
                            .unwrap_or_default();
                        app.selected_tx_op_idx = 0;
                    }
                }
            }
        },
        KeyCode::PageDown | KeyCode::Char('J') => {
            if app.focus == PaneFocus::Right && !app.selected_tx_ops.is_empty() {
                app.selected_tx_op_idx =
                    (app.selected_tx_op_idx + 1).min(app.selected_tx_ops.len() - 1);
            } else if app.focus == PaneFocus::Left && !app.staging.is_empty() {
                app.selected_staging_idx =
                    (app.selected_staging_idx + 10).min(app.staging.len() - 1);
            }
        }
        KeyCode::PageUp | KeyCode::Char('K') => {
            if app.focus == PaneFocus::Right && !app.selected_tx_ops.is_empty() {
                app.selected_tx_op_idx = app.selected_tx_op_idx.saturating_sub(1);
            } else if app.focus == PaneFocus::Left && !app.staging.is_empty() {
                app.selected_staging_idx = app.selected_staging_idx.saturating_sub(10);
            }
        }
        KeyCode::Char('d') | KeyCode::Delete => {
            if app.focus == PaneFocus::Left && !app.staging.is_empty() {
                app.staging.remove_at(app.selected_staging_idx);
                if app.selected_staging_idx >= app.staging.len() && !app.staging.is_empty() {
                    app.selected_staging_idx = app.staging.len() - 1;
                }
            }
        }
        KeyCode::Char('X') => {
            app.staging.clear();
            app.selected_staging_idx = 0;
            app.status_toast = Some(("Cola de Staging vaciada".to_string(), false));
        }
        KeyCode::Char('R') => {
            if let Some(tx) = app.transactions.get(app.selected_tx_idx) {
                app.modal = Modal::ConfirmRevertTransaction {
                    tx_id: tx.id,
                    only_op_id: None,
                    summary: tx.summary.clone(),
                };
            }
        }
        KeyCode::Char('r') => {
            if let Some(tx) = app.transactions.get(app.selected_tx_idx) {
                if let Some(op) = app.selected_tx_ops.get(app.selected_tx_op_idx) {
                    app.modal = Modal::ConfirmRevertTransaction {
                        tx_id: tx.id,
                        only_op_id: Some(op.id),
                        summary: op.target_label.clone(),
                    };
                }
            }
        }
        _ => {}
    }
}

fn handle_modal_key(app: &mut App, key: KeyEvent) {
    let current_modal = app.modal.clone();
    match current_modal {
        Modal::None => {}
        Modal::Help => {
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?') | KeyCode::Char('q')
            ) {
                app.modal = Modal::None;
            }
        }
        Modal::TextInput {
            title,
            prompt,
            mut value,
            kind,
        } => match key.code {
            KeyCode::Esc => app.modal = Modal::None,
            KeyCode::Backspace => {
                value.pop();
                app.modal = Modal::TextInput {
                    title,
                    prompt,
                    value,
                    kind,
                };
            }
            KeyCode::Char(c) => {
                value.push(c);
                app.modal = Modal::TextInput {
                    title,
                    prompt,
                    value,
                    kind,
                };
            }
            KeyCode::Enter => {
                app.modal = Modal::None;
                let trimmed = value.trim().to_string();
                match kind {
                    TextInputKind::FilterSitesLocal => {
                        app.site_filter = trimmed;
                        app.selected_site_idx = 0;
                    }
                    TextInputKind::SearchSitesTenant => {
                        app.trigger_discover_sites(if trimmed.is_empty() {
                            None
                        } else {
                            Some(trimmed)
                        });
                    }
                    TextInputKind::ManualSiteUrl => {
                        if !trimmed.is_empty() {
                            app.trigger_check_manual_site(trimmed);
                        }
                    }
                    TextInputKind::FilterCurrentPane => {
                        app.pane_filter = trimmed;
                    }
                    TextInputKind::CreateSharePointGroup => {
                        if !trimmed.is_empty() {
                            if let Some(ref site) = app.current_site {
                                app.staging.push(StagingOp::CreateGroup {
                                    site_url: site.url.clone(),
                                    group_title: trimmed.clone(),
                                    description: "Creado desde SPOIberia TUI".to_string(),
                                });
                                app.status_toast = Some((
                                    format!(
                                        "Encolada creación del grupo '{}'. Pulsa 'A' para Aplicar.",
                                        trimmed
                                    ),
                                    false,
                                ));
                            }
                        }
                    }
                    TextInputKind::CreateDocumentLibrary => {
                        if !trimmed.is_empty() {
                            if let Some(ref site) = app.current_site {
                                app.staging.push(StagingOp::CreateLibrary {
                                    site_url: site.url.clone(),
                                    library_title: trimmed.clone(),
                                    description: "Biblioteca creada desde SPOIberia".to_string(),
                                });
                                app.status_toast = Some((
                                    format!(
                                        "Encolada creación de biblioteca '{}'. Pulsa 'A' para Aplicar.",
                                        trimmed
                                    ),
                                    false,
                                ));
                            }
                        }
                    }
                    TextInputKind::CreateFolderRelativePath => {
                        if !trimmed.is_empty() {
                            if let Some(ref site) = app.current_site {
                                let lib_title = app
                                    .selected_library()
                                    .map(|l| l.title.clone())
                                    .unwrap_or_else(|| "Documents".to_string());
                                app.staging.push(StagingOp::CreateFolder {
                                    site_url: site.url.clone(),
                                    library_title: lib_title.clone(),
                                    relative_folder_path: trimmed.clone(),
                                });
                                app.status_toast = Some((
                                    format!(
                                        "Encolada creación de carpeta '{}/{}'. Pulsa 'A' para Aplicar.",
                                        lib_title, trimmed
                                    ),
                                    false,
                                ));
                            }
                        }
                    }
                    TextInputKind::ImportMatrixFilePath => {
                        if !trimmed.is_empty() {
                            app.trigger_import_matrix_file(trimmed);
                        }
                    }
                    TextInputKind::ExportMatrixFilePath => {
                        if !trimmed.is_empty() {
                            app.trigger_export_matrix_file(trimmed);
                        }
                    }
                    TextInputKind::SetCloneSourceUrl => {
                        app.clone_source_url = trimmed;
                    }
                }
            }
            _ => {}
        },
        Modal::AddUserToGroup {
            group_id,
            group_title,
            mut query,
            mut results,
            mut selected_idx,
            mut marked_identifiers,
            mut searching,
        } => match key.code {
            KeyCode::Esc => app.modal = Modal::None,
            KeyCode::Down
            | KeyCode::Up
            | KeyCode::PageDown
            | KeyCode::PageUp
            | KeyCode::Home
            | KeyCode::End => {
                if !results.is_empty() {
                    selected_idx = move_list_index(selected_idx, results.len(), key.code);
                }
                app.modal = Modal::AddUserToGroup {
                    group_id,
                    group_title,
                    query,
                    results,
                    selected_idx,
                    marked_identifiers,
                    searching,
                };
            }
            KeyCode::Tab | KeyCode::Insert => {
                if !results.is_empty() {
                    if let Some(u) = results.get(selected_idx) {
                        let ident = u.identifier().to_string();
                        if !marked_identifiers.remove(&ident) {
                            marked_identifiers.insert(ident);
                        }
                    }
                }
                app.modal = Modal::AddUserToGroup {
                    group_id,
                    group_title,
                    query,
                    results,
                    selected_idx,
                    marked_identifiers,
                    searching,
                };
            }
            KeyCode::Backspace => {
                query.pop();
                let should_search = query.trim().len() >= 2;
                let q_clone = query.clone();
                searching = should_search;
                if !should_search {
                    results.clear();
                    selected_idx = 0;
                }
                app.modal = Modal::AddUserToGroup {
                    group_id,
                    group_title,
                    query,
                    results,
                    selected_idx,
                    marked_identifiers,
                    searching,
                };
                if should_search {
                    app.trigger_search_directory_users(q_clone);
                }
            }
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) && c == 's' {
                    if !query.trim().is_empty() {
                        searching = true;
                        let q_clone = query.clone();
                        app.modal = Modal::AddUserToGroup {
                            group_id,
                            group_title,
                            query,
                            results,
                            selected_idx,
                            marked_identifiers,
                            searching,
                        };
                        app.trigger_search_directory_users(q_clone);
                    }
                    return;
                }
                query.push(c);
                let should_search = !c.is_whitespace() && query.trim().len() >= 2;
                let q_clone = query.clone();
                searching = should_search;
                app.modal = Modal::AddUserToGroup {
                    group_id,
                    group_title,
                    query,
                    results,
                    selected_idx,
                    marked_identifiers,
                    searching,
                };
                if should_search {
                    app.trigger_search_directory_users(q_clone);
                }
            }
            KeyCode::Enter => {
                let Some(ref site) = app.current_site else {
                    app.modal = Modal::None;
                    return;
                };
                let mut added_cnt = 0usize;
                if !marked_identifiers.is_empty() {
                    for ident in &marked_identifiers {
                        let disp = results
                            .iter()
                            .find(|r| r.identifier() == ident)
                            .map(|r| r.display_name.clone())
                            .unwrap_or_else(|| ident.clone());
                        app.staging.push(StagingOp::AddUserToGroup {
                            site_url: site.url.clone(),
                            group_id,
                            group_title: group_title.clone(),
                            user_identifier: ident.clone(),
                            user_display_name: disp,
                        });
                        added_cnt += 1;
                    }
                } else if let Some(u) = results.get(selected_idx) {
                    app.staging.push(StagingOp::AddUserToGroup {
                        site_url: site.url.clone(),
                        group_id,
                        group_title: group_title.clone(),
                        user_identifier: u.identifier().to_string(),
                        user_display_name: u.display_name.clone(),
                    });
                    added_cnt = 1;
                } else if !query.trim().is_empty() {
                    let raw_ident = query.trim().to_string();
                    app.staging.push(StagingOp::AddUserToGroup {
                        site_url: site.url.clone(),
                        group_id,
                        group_title: group_title.clone(),
                        user_identifier: raw_ident.clone(),
                        user_display_name: raw_ident,
                    });
                    added_cnt = 1;
                }
                app.modal = Modal::None;
                if added_cnt > 0 {
                    app.status_toast = Some((
                        format!(
                            "✓ {} usuario(s) encolados para añadir a '{}'. Pulsa 'A' para Aplicar.",
                            added_cnt, group_title
                        ),
                        false,
                    ));
                }
            }
            _ => {}
        },
        Modal::GrantFolderPermission {
            library_title,
            relative_folder_path,
            mut group_name,
            mut role_idx,
            mut copy_parent_roles,
            mut clear_subscopes,
            create_group_if_missing,
            mut active_field,
        } => match key.code {
            KeyCode::Esc => app.modal = Modal::None,
            KeyCode::Tab | KeyCode::Down => {
                active_field = (active_field + 1) % 4;
                app.modal = Modal::GrantFolderPermission {
                    library_title,
                    relative_folder_path,
                    group_name,
                    role_idx,
                    copy_parent_roles,
                    clear_subscopes,
                    create_group_if_missing,
                    active_field,
                };
            }
            KeyCode::Up => {
                active_field = active_field.checked_sub(1).unwrap_or(3);
                app.modal = Modal::GrantFolderPermission {
                    library_title,
                    relative_folder_path,
                    group_name,
                    role_idx,
                    copy_parent_roles,
                    clear_subscopes,
                    create_group_if_missing,
                    active_field,
                };
            }
            KeyCode::Left if active_field == 1 => {
                role_idx = role_idx.checked_sub(1).unwrap_or(STANDARD_ROLES.len() - 1);
                app.modal = Modal::GrantFolderPermission {
                    library_title,
                    relative_folder_path,
                    group_name,
                    role_idx,
                    copy_parent_roles,
                    clear_subscopes,
                    create_group_if_missing,
                    active_field,
                };
            }
            KeyCode::Right if active_field == 1 => {
                role_idx = (role_idx + 1) % STANDARD_ROLES.len();
                app.modal = Modal::GrantFolderPermission {
                    library_title,
                    relative_folder_path,
                    group_name,
                    role_idx,
                    copy_parent_roles,
                    clear_subscopes,
                    create_group_if_missing,
                    active_field,
                };
            }
            KeyCode::Char(' ') if active_field >= 2 => {
                if active_field == 2 {
                    copy_parent_roles = !copy_parent_roles;
                } else if active_field == 3 {
                    clear_subscopes = !clear_subscopes;
                }
                app.modal = Modal::GrantFolderPermission {
                    library_title,
                    relative_folder_path,
                    group_name,
                    role_idx,
                    copy_parent_roles,
                    clear_subscopes,
                    create_group_if_missing,
                    active_field,
                };
            }
            KeyCode::Backspace if active_field == 0 => {
                group_name.pop();
                app.modal = Modal::GrantFolderPermission {
                    library_title,
                    relative_folder_path,
                    group_name,
                    role_idx,
                    copy_parent_roles,
                    clear_subscopes,
                    create_group_if_missing,
                    active_field,
                };
            }
            KeyCode::Char(c) if active_field == 0 => {
                group_name.push(c);
                app.modal = Modal::GrantFolderPermission {
                    library_title,
                    relative_folder_path,
                    group_name,
                    role_idx,
                    copy_parent_roles,
                    clear_subscopes,
                    create_group_if_missing,
                    active_field,
                };
            }
            KeyCode::Enter => {
                let grp = group_name.trim().to_string();
                if grp.is_empty() {
                    app.status_toast = Some(("El nombre del grupo no puede estar vacío".to_string(), true));
                    return;
                }
                let role_name = STANDARD_ROLES
                    .get(role_idx)
                    .copied()
                    .unwrap_or("Read")
                    .to_string();
                if let Some(ref site) = app.current_site {
                    if create_group_if_missing {
                        app.staging.push(StagingOp::CreateGroup {
                            site_url: site.url.clone(),
                            group_title: grp.clone(),
                            description: format!("Grupo para carpeta {}", relative_folder_path),
                        });
                    }
                    app.staging.push(StagingOp::BreakFolderInheritance {
                        site_url: site.url.clone(),
                        library_title: library_title.clone(),
                        relative_folder_path: relative_folder_path.clone(),
                        copy_parent_roles,
                        clear_subscopes,
                    });
                    app.staging.push(StagingOp::AssignFolderRole {
                        site_url: site.url.clone(),
                        library_title: library_title.clone(),
                        relative_folder_path: relative_folder_path.clone(),
                        principal_name: grp.clone(),
                        principal_type: "SharePointGroup".to_string(),
                        role_name: role_name.clone(),
                        create_group_if_missing,
                    });
                    app.status_toast = Some((
                        format!(
                            "✓ Permiso [{}] para '{}' en '{}' encolado en Staging. Pulsa 'A' para Aplicar.",
                            role_name, grp, relative_folder_path
                        ),
                        false,
                    ));
                }
                app.modal = Modal::None;
            }
            _ => {}
        },
        Modal::ConfirmApplyStaging { mut note } => match key.code {
            KeyCode::Esc => app.modal = Modal::None,
            KeyCode::Backspace => {
                note.pop();
                app.modal = Modal::ConfirmApplyStaging { note };
            }
            KeyCode::Char(c) => {
                note.push(c);
                app.modal = Modal::ConfirmApplyStaging { note };
            }
            KeyCode::Enter => {
                app.modal = Modal::None;
                let opt_note = if note.trim().is_empty() {
                    None
                } else {
                    Some(note.trim().to_string())
                };
                app.trigger_apply_staging(opt_note);
            }
            _ => {}
        },
        Modal::ConfirmRevertTransaction {
            tx_id,
            only_op_id,
            ..
        } => match key.code {
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                app.modal = Modal::None;
            }
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.modal = Modal::None;
                app.trigger_revert_transaction(tx_id, only_op_id);
            }
            _ => {}
        },
    }
}
