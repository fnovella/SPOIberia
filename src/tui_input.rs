use crate::app::{
    App, Modal, PaneFocus, Screen, SyncMenuScope, TextInputKind, WorkspaceTab, STANDARD_ROLES,
};
use crate::i18n::Lang;
use crate::models::{analyze_permission_change_impact, StagingOp};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashSet;

pub fn handle_key_event(app: &mut App, key: KeyEvent) {
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
                title: app
                    .lang
                    .tr("Filtrar Sitios Localmente", "Filter Sites Locally")
                    .to_string(),
                prompt: app
                    .lang
                    .tr(
                        "Escribe parte del título, URL o rol (deja vacío para mostrar todos):",
                        "Type part of the title, URL or role (leave empty to show all):",
                    )
                    .to_string(),
                value: app.site_filter.clone(),
                kind: TextInputKind::FilterSitesLocal,
            };
        }
        KeyCode::Char('s') => {
            app.modal = Modal::TextInput {
                title: app
                    .lang
                    .tr(
                        "Buscar Sitios en el Tenant M365",
                        "Search Sites in M365 Tenant",
                    )
                    .to_string(),
                prompt: app
                    .lang
                    .tr(
                        "Palabra clave a buscar en SharePoint Search (verifica rol Admin/Owner):",
                        "Keyword to search in SharePoint Search (verifies Admin/Owner role):",
                    )
                    .to_string(),
                value: String::new(),
                kind: TextInputKind::SearchSitesTenant,
            };
        }
        KeyCode::Char('u') => {
            app.modal = Modal::TextInput {
                title: app
                    .lang
                    .tr("Abrir Sitio SharePoint por URL", "Open SharePoint Site by URL")
                    .to_string(),
                prompt: app
                    .lang
                    .tr(
                        "Introduce la URL completa del sitio SharePoint:",
                        "Enter the full SharePoint site URL:",
                    )
                    .to_string(),
                value: "https://".to_string(),
                kind: TextInputKind::ManualSiteUrl,
            };
        }
        KeyCode::Char('c') => {
            if let Some(site) = app.selected_site() {
                app.clone_source_url = site.url.clone();
                app.status_toast = Some((
                    match app.lang {
                        Lang::Es => format!(
                            "✓ Sitio '{}' fijado como Origen para Clonado (Fase 3)",
                            site.title
                        ),
                        Lang::En => format!(
                            "✓ Site '{}' set as Clone Source (Phase 3)",
                            site.title
                        ),
                    },
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
        KeyCode::Backspace => {
            if app.tab == WorkspaceTab::LibrariesAndFolders && app.navigate_up_folder() {
                return;
            }
            app.screen = Screen::SiteSelector;
            return;
        }
        KeyCode::Esc => {
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
                    app.lang
                        .tr(
                            "No hay cambios pendientes en la cola de Staging para aplicar",
                            "No pending changes in Staging queue to apply",
                        )
                        .to_string(),
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
            app.open_sync_menu(SyncMenuScope::SiteAndLibraries);
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
                    match app.lang {
                        Lang::Es => format!(
                            "Encolada baja de '{}' en '{}'. Pulsa 'A' para Aplicar.",
                            member.title, grp.title
                        ),
                        Lang::En => format!(
                            "Queued removal of '{}' from '{}'. Press 'A' to Apply.",
                            member.title, grp.title
                        ),
                    },
                    false,
                ));
            }
        }
        KeyCode::Char('g') => {
            app.modal = Modal::TextInput {
                title: app
                    .lang
                    .tr(
                        "Crear Nuevo Grupo de SharePoint",
                        "Create New SharePoint Group",
                    )
                    .to_string(),
                prompt: app
                    .lang
                    .tr(
                        "Nombre del nuevo grupo de SharePoint a crear en el sitio:",
                        "Name of the new SharePoint group to create on the site:",
                    )
                    .to_string(),
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
                        app.current_folder_path.clear();
                        app.folder_ancestors.clear();
                        if let Some(lib_title) = app.selected_library().map(|l| l.title.clone()) {
                            app.trigger_load_library_folders(lib_title, None, None);
                        }
                    }
                }
            }
            PaneFocus::Right => {
                let total = app.folder_list_row_count();
                if total > 0 {
                    app.selected_folder_idx =
                        move_list_index(app.selected_folder_idx, total, key.code);
                }
            }
        },
        KeyCode::Enter => match app.focus {
            PaneFocus::Left => {
                app.focus = PaneFocus::Right;
            }
            PaneFocus::Right => {
                app.navigate_into_selected_folder();
            }
        },
        KeyCode::Char('u') => {
            app.navigate_up_folder();
        }
        KeyCode::Char('n') => {
            let lib_title = app
                .selected_library()
                .map(|l| l.title.as_str())
                .unwrap_or("Documents");
            let current_loc = if app.current_folder_path.is_empty() {
                lib_title.to_string()
            } else {
                format!("{}/{}", lib_title, app.current_folder_path)
            };
            app.modal = Modal::TextInput {
                title: match app.lang {
                    Lang::Es => format!("Crear Nueva Carpeta en '{}'", current_loc),
                    Lang::En => format!("Create New Folder in '{}'", current_loc),
                },
                prompt: app
                    .lang
                    .tr(
                        "Nombre o subruta (relativa al nivel actual; empieza con '/' para raíz):",
                        "Name or subpath (relative to current level; start with '/' for root):",
                    )
                    .to_string(),
                value: String::new(),
                kind: TextInputKind::CreateFolderRelativePath,
            };
        }
        KeyCode::Char('b') => {
            app.modal = Modal::TextInput {
                title: app
                    .lang
                    .tr(
                        "Crear Nueva Biblioteca de Documentos",
                        "Create New Document Library",
                    )
                    .to_string(),
                prompt: app
                    .lang
                    .tr(
                        "Título de la nueva Biblioteca de Documentos:",
                        "Title of the new Document Library:",
                    )
                    .to_string(),
                value: String::new(),
                kind: TextInputKind::CreateDocumentLibrary,
            };
        }
        KeyCode::Char('o') => {
            app.open_sync_menu(SyncMenuScope::SelectedLibrary);
        }
        KeyCode::Char('p') => {
            let Some((
                lib_title,
                rel_path,
                target_has_unique_roles,
                target_current_roles,
                inheritance,
            )) = app.target_folder_context()
            else {
                app.status_toast = Some((
                    app.lang
                        .tr(
                            "Selecciona o crea primero una carpeta para asignarle permisos",
                            "Select or create a folder first to grant permissions",
                        )
                        .to_string(),
                    true,
                ));
                return;
            };
            app.modal = Modal::GrantFolderPermission {
                library_title: lib_title,
                relative_folder_path: rel_path,
                target_has_unique_roles,
                target_current_roles,
                inheritance,
                group_name: String::new(),
                role_idx: 2, // Default "Edit"
                copy_parent_roles: true,
                replace_inherited_roles: true,
                clear_subscopes: false,
                create_group_if_missing: true,
                active_field: 0,
            };
        }
        KeyCode::Char('d') | KeyCode::Delete => {
            let Some((
                lib_title,
                rel_path,
                target_has_unique_roles,
                target_current_roles,
                inheritance,
            )) = app.target_folder_context()
            else {
                app.status_toast = Some((
                    app.lang
                        .tr(
                            "Selecciona primero una carpeta para gestionar sus permisos",
                            "Select a folder first to manage its permissions",
                        )
                        .to_string(),
                    true,
                ));
                return;
            };
            let source_entries = if target_has_unique_roles {
                &target_current_roles
            } else {
                &inheritance.inherited_roles
            };
            let mut entries = Vec::new();
            for entry in source_entries {
                for role in &entry.roles {
                    entries.push((
                        entry.principal_title.clone(),
                        entry.principal_type.clone(),
                        role.clone(),
                    ));
                }
            }
            if entries.is_empty() {
                app.status_toast = Some((
                    app.lang
                        .tr(
                            "No hay asignaciones de permisos visibles para revocar en esta carpeta",
                            "No visible permission assignments to revoke on this folder",
                        )
                        .to_string(),
                    true,
                ));
                return;
            }
            app.scroll.modal_folder_perms.set(0);
            app.modal = Modal::ManageFolderPermissions {
                library_title: lib_title,
                relative_folder_path: rel_path,
                target_has_unique_roles,
                inheritance,
                entries,
                selected_idx: 0,
            };
        }
        KeyCode::Char('x') => {
            let Some((lib_title, rel_path, target_has_unique_roles, _, inheritance)) =
                app.target_folder_context()
            else {
                app.status_toast = Some((
                    app.lang
                        .tr(
                            "Selecciona primero una carpeta para restaurar su herencia",
                            "Select a folder first to restore its inheritance",
                        )
                        .to_string(),
                    true,
                ));
                return;
            };
            if !target_has_unique_roles {
                app.status_toast = Some((
                    match app.lang {
                        Lang::Es => format!(
                            "La carpeta '{}' ya hereda permisos de {}",
                            rel_path, inheritance.inherited_from_label
                        ),
                        Lang::En => format!(
                            "Folder '{}' already inherits permissions from {}",
                            rel_path, inheritance.inherited_from_label
                        ),
                    },
                    false,
                ));
                return;
            }
            app.modal = Modal::ConfirmResetFolderInheritance {
                library_title: lib_title,
                relative_folder_path: rel_path,
                inheritance,
            };
        }
        KeyCode::Char('i') => {
            app.modal = Modal::TextInput {
                title: app
                    .lang
                    .tr(
                        "Importar Matriz de Carpetas y Permisos (CSV / JSON)",
                        "Import Folders & Permissions Matrix (CSV / JSON)",
                    )
                    .to_string(),
                prompt: app
                    .lang
                    .tr(
                        "Ruta del archivo externo (.csv o .json):",
                        "Path to external file (.csv or .json):",
                    )
                    .to_string(),
                value: "./examples/sample-permissions.csv".to_string(),
                kind: TextInputKind::ImportMatrixFilePath,
            };
        }
        KeyCode::Char('e') => {
            app.modal = Modal::TextInput {
                title: app
                    .lang
                    .tr(
                        "Exportar Matriz de Permisos Únicos (CSV / JSON)",
                        "Export Unique Permissions Matrix (CSV / JSON)",
                    )
                    .to_string(),
                prompt: app
                    .lang
                    .tr(
                        "Ruta del archivo de salida (.csv o .json):",
                        "Output file path (.csv or .json):",
                    )
                    .to_string(),
                value: "./exported-permissions.json".to_string(),
                kind: TextInputKind::ExportMatrixFilePath,
            };
        }
        KeyCode::Char('r') => {
            if let Some(lib_title) = app.selected_library().map(|l| l.title.clone()) {
                let sub = if app.current_folder_path.is_empty() {
                    None
                } else {
                    Some(app.current_folder_path.clone())
                };
                app.trigger_load_library_folders(lib_title, sub, None);
            }
        }
        _ => {}
    }
}

fn handle_tab_clone_keys(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => {
            app.clone_selected_option = (app.clone_selected_option + 1) % 5;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.clone_selected_option = app.clone_selected_option.checked_sub(1).unwrap_or(4);
        }
        KeyCode::Char(' ') => match app.clone_selected_option {
            0 => app.clone_include_groups = !app.clone_include_groups,
            1 => app.clone_include_files = !app.clone_include_files,
            2 => app.clone_include_unique_perms = !app.clone_include_unique_perms,
            3 => app.clone_overwrite = !app.clone_overwrite,
            4 => app.clone_destructive = !app.clone_destructive,
            _ => {}
        },
        KeyCode::Char('s') => {
            app.modal = Modal::TextInput {
                title: app
                    .lang
                    .tr(
                        "Sitio SharePoint Origen para Clonado",
                        "Source SharePoint Site for Cloning",
                    )
                    .to_string(),
                prompt: app
                    .lang
                    .tr(
                        "URL del sitio SharePoint Origen desde el que copiar grupos, datos y permisos:",
                        "URL of the Source SharePoint site to copy groups, data and permissions from:",
                    )
                    .to_string(),
                value: app.clone_source_url.clone(),
                kind: TextInputKind::SetCloneSourceUrl,
            };
        }
        KeyCode::Enter => {
            if app.clone_destructive {
                if app.clone_source_url.trim().is_empty() {
                    app.status_toast = Some((
                        app.lang
                            .tr(
                                "Indica primero la URL del sitio SharePoint Origen (pulsa 's')",
                                "Enter the Source SharePoint site URL first (press 's')",
                            )
                            .to_string(),
                        true,
                    ));
                    return;
                }
                let target_url = app
                    .current_site
                    .as_ref()
                    .map(|s| s.url.clone())
                    .unwrap_or_default();
                app.modal = Modal::ConfirmDestructiveClone {
                    source_url: app.clone_source_url.trim().to_string(),
                    target_url,
                    typed_confirm: String::new(),
                };
            } else {
                app.trigger_plan_clone();
            }
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
            app.status_toast = Some((
                app.lang
                    .tr("Cola de Staging vaciada", "Staging queue cleared")
                    .to_string(),
                false,
            ));
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
    match &mut app.modal {
        Modal::None => {}
        Modal::Help => {
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?') | KeyCode::Char('q')
            ) {
                app.modal = Modal::None;
            }
        }
        Modal::TextInput { value, kind, .. } => match key.code {
            KeyCode::Esc => app.modal = Modal::None,
            KeyCode::Backspace => {
                value.pop();
            }
            KeyCode::Char(c) => {
                value.push(c);
            }
            KeyCode::Enter => {
                let trimmed = value.trim().to_string();
                let kind = *kind;
                app.modal = Modal::None;
                submit_text_input_modal(app, trimmed, kind);
            }
            _ => {}
        },
        Modal::AddUserToGroup {
            group_id,
            group_title,
            query,
            results,
            selected_idx,
            marked_identifiers,
            searching,
        } => match key.code {
            KeyCode::Esc => app.modal = Modal::None,
            KeyCode::Down
            | KeyCode::Up
            | KeyCode::PageDown
            | KeyCode::PageUp
            | KeyCode::Home
            | KeyCode::End => {
                if !results.is_empty() {
                    *selected_idx = move_list_index(*selected_idx, results.len(), key.code);
                }
            }
            KeyCode::Tab | KeyCode::Insert => {
                if let Some(u) = results.get(*selected_idx) {
                    let ident = u.identifier().to_string();
                    if !marked_identifiers.remove(&ident) {
                        marked_identifiers.insert(ident);
                    }
                }
            }
            KeyCode::Backspace => {
                query.pop();
                let should_search = query.trim().len() >= 2;
                let q_clone = query.clone();
                *searching = should_search;
                if !should_search {
                    results.clear();
                    *selected_idx = 0;
                }
                if should_search {
                    app.trigger_search_directory_users(q_clone);
                }
            }
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) && c == 's' {
                    if !query.trim().is_empty() {
                        *searching = true;
                        let q_clone = query.clone();
                        app.trigger_search_directory_users(q_clone);
                    }
                    return;
                }
                query.push(c);
                let should_search = !c.is_whitespace() && query.trim().len() >= 2;
                let q_clone = query.clone();
                *searching = should_search;
                if should_search {
                    app.trigger_search_directory_users(q_clone);
                }
            }
            KeyCode::Enter => {
                let group_id = *group_id;
                let group_title = group_title.clone();
                let query = query.clone();
                let results = std::mem::take(results);
                let selected_idx = *selected_idx;
                let marked_identifiers = std::mem::take(marked_identifiers);
                app.modal = Modal::None;

                let Some(ref site) = app.current_site else {
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
                if added_cnt > 0 {
                    app.status_toast = Some((
                        match app.lang {
                            Lang::Es => format!(
                                "✓ {} usuario(s) encolados para añadir a '{}'. Pulsa 'A' para Aplicar.",
                                added_cnt, group_title
                            ),
                            Lang::En => format!(
                                "✓ {} user(s) queued to add to '{}'. Press 'A' to Apply.",
                                added_cnt, group_title
                            ),
                        },
                        false,
                    ));
                }
            }
            _ => {}
        },
        Modal::GrantFolderPermission {
            library_title,
            relative_folder_path,
            target_has_unique_roles,
            target_current_roles,
            inheritance,
            group_name,
            role_idx,
            copy_parent_roles,
            replace_inherited_roles,
            clear_subscopes,
            create_group_if_missing,
            active_field,
        } => match key.code {
            KeyCode::Esc => app.modal = Modal::None,
            KeyCode::Tab | KeyCode::Down => {
                *active_field = (*active_field + 1) % 5;
            }
            KeyCode::Up => {
                *active_field = active_field.checked_sub(1).unwrap_or(4);
            }
            KeyCode::Left if *active_field == 1 => {
                *role_idx = role_idx.checked_sub(1).unwrap_or(STANDARD_ROLES.len() - 1);
            }
            KeyCode::Right if *active_field == 1 => {
                *role_idx = (*role_idx + 1) % STANDARD_ROLES.len();
            }
            KeyCode::Char(' ') if *active_field >= 2 => {
                if *active_field == 2 {
                    *copy_parent_roles = !*copy_parent_roles;
                } else if *active_field == 3 {
                    *replace_inherited_roles = !*replace_inherited_roles;
                } else if *active_field == 4 {
                    *clear_subscopes = !*clear_subscopes;
                }
            }
            KeyCode::Backspace if *active_field == 0 => {
                group_name.pop();
            }
            KeyCode::Char(c) if *active_field == 0 => {
                group_name.push(c);
            }
            KeyCode::Enter => {
                let grp = group_name.trim().to_string();
                if grp.is_empty() {
                    app.status_toast = Some((
                        app.lang
                            .tr(
                                "El nombre del grupo no puede estar vacío",
                                "Group name cannot be empty",
                            )
                            .to_string(),
                        true,
                    ));
                    return;
                }
                let role_name = STANDARD_ROLES
                    .get(*role_idx)
                    .copied()
                    .unwrap_or("Read")
                    .to_string();
                let library_title = library_title.clone();
                let relative_folder_path = relative_folder_path.clone();
                let target_has_unique_roles = *target_has_unique_roles;
                let target_current_roles = std::mem::take(target_current_roles);
                let inheritance = inheritance.clone();
                let copy_parent_roles = *copy_parent_roles;
                let replace_inherited_roles = *replace_inherited_roles;
                let clear_subscopes = *clear_subscopes;
                let create_group_if_missing = *create_group_if_missing;
                app.modal = Modal::None;

                let impact = analyze_permission_change_impact(
                    target_has_unique_roles,
                    &target_current_roles,
                    &inheritance,
                    &grp,
                    &role_name,
                    copy_parent_roles,
                    clear_subscopes,
                );
                let matched_principal_type = if target_has_unique_roles {
                    &target_current_roles
                } else {
                    &inheritance.inherited_roles
                }
                .iter()
                .find(|e| {
                    e.principal_title.eq_ignore_ascii_case(&grp)
                        || e.principal_login_name.eq_ignore_ascii_case(&grp)
                })
                .map(|e| e.principal_type.clone())
                .unwrap_or_else(|| "SharePointGroup".to_string());

                if let Some(ref site) = app.current_site {
                    if create_group_if_missing {
                        app.staging.push(StagingOp::CreateGroup {
                            site_url: site.url.clone(),
                            group_title: grp.clone(),
                            description: format!("Grupo para carpeta {}", relative_folder_path),
                        });
                    }
                    if !target_has_unique_roles || clear_subscopes {
                        app.staging.push(StagingOp::BreakFolderInheritance {
                            site_url: site.url.clone(),
                            library_title: library_title.clone(),
                            relative_folder_path: relative_folder_path.clone(),
                            copy_parent_roles,
                            clear_subscopes,
                        });
                    }
                    if replace_inherited_roles {
                        for old_role in &impact.conflicting_roles {
                            app.staging.push(StagingOp::RemoveFolderRole {
                                site_url: site.url.clone(),
                                library_title: library_title.clone(),
                                relative_folder_path: relative_folder_path.clone(),
                                principal_name: grp.clone(),
                                principal_type: matched_principal_type.clone(),
                                role_name: old_role.clone(),
                            });
                        }
                    }
                    app.staging.push(StagingOp::AssignFolderRole {
                        site_url: site.url.clone(),
                        library_title,
                        relative_folder_path: relative_folder_path.clone(),
                        principal_name: grp.clone(),
                        principal_type: matched_principal_type,
                        role_name: role_name.clone(),
                        create_group_if_missing,
                    });
                    app.status_toast = Some((
                        match app.lang {
                            Lang::Es => format!(
                                "✓ Permiso [{}] para '{}' en '{}' encolado en Staging. Pulsa 'A' para Aplicar.",
                                role_name, grp, relative_folder_path
                            ),
                            Lang::En => format!(
                                "✓ Permission [{}] for '{}' on '{}' queued in Staging. Press 'A' to Apply.",
                                role_name, grp, relative_folder_path
                            ),
                        },
                        false,
                    ));
                }
            }
            _ => {}
        },
        Modal::ManageFolderPermissions {
            library_title,
            relative_folder_path,
            target_has_unique_roles,
            entries,
            selected_idx,
            ..
        } => match key.code {
            KeyCode::Esc => app.modal = Modal::None,
            KeyCode::Down
            | KeyCode::Char('j')
            | KeyCode::Up
            | KeyCode::Char('k')
            | KeyCode::PageDown
            | KeyCode::PageUp
            | KeyCode::Home
            | KeyCode::End => {
                if !entries.is_empty() {
                    *selected_idx = move_list_index(*selected_idx, entries.len(), key.code);
                }
            }
            KeyCode::Enter | KeyCode::Char('d') | KeyCode::Delete => {
                let library_title = library_title.clone();
                let relative_folder_path = relative_folder_path.clone();
                let target_has_unique_roles = *target_has_unique_roles;
                let selected_entry = entries.get(*selected_idx).cloned();
                app.modal = Modal::None;

                if let (Some(site), Some((principal_name, principal_type, role_name))) =
                    (app.current_site.as_ref(), selected_entry)
                {
                    if !target_has_unique_roles {
                        app.staging.push(StagingOp::BreakFolderInheritance {
                            site_url: site.url.clone(),
                            library_title: library_title.clone(),
                            relative_folder_path: relative_folder_path.clone(),
                            copy_parent_roles: true,
                            clear_subscopes: false,
                        });
                    }
                    app.staging.push(StagingOp::RemoveFolderRole {
                        site_url: site.url.clone(),
                        library_title,
                        relative_folder_path: relative_folder_path.clone(),
                        principal_name: principal_name.clone(),
                        principal_type,
                        role_name: role_name.clone(),
                    });
                    app.status_toast = Some((
                        match app.lang {
                            Lang::Es => format!(
                                "✓ Encolada revocación de '{}' para '{}' en '{}'. Pulsa 'A' para Aplicar.",
                                role_name, principal_name, relative_folder_path
                            ),
                            Lang::En => format!(
                                "✓ Queued revocation of '{}' from '{}' on '{}'. Press 'A' to Apply.",
                                role_name, principal_name, relative_folder_path
                            ),
                        },
                        false,
                    ));
                }
            }
            _ => {}
        },
        Modal::ConfirmResetFolderInheritance {
            library_title,
            relative_folder_path,
            ..
        } => match key.code {
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                app.modal = Modal::None;
            }
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                let library_title = library_title.clone();
                let relative_folder_path = relative_folder_path.clone();
                app.modal = Modal::None;
                queue_reset_folder_inheritance(app, library_title, relative_folder_path);
            }
            KeyCode::Char('s') | KeyCode::Char('S') if app.lang == Lang::Es => {
                let library_title = library_title.clone();
                let relative_folder_path = relative_folder_path.clone();
                app.modal = Modal::None;
                queue_reset_folder_inheritance(app, library_title, relative_folder_path);
            }
            _ => {}
        },
        Modal::ConfirmApplyStaging { note } => match key.code {
            KeyCode::Esc => app.modal = Modal::None,
            KeyCode::Backspace => {
                note.pop();
            }
            KeyCode::Char(c) => {
                note.push(c);
            }
            KeyCode::Enter => {
                let opt_note = if note.trim().is_empty() {
                    None
                } else {
                    Some(note.trim().to_string())
                };
                app.modal = Modal::None;
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
                let (tx_id, only_op_id) = (*tx_id, *only_op_id);
                app.modal = Modal::None;
                app.trigger_revert_transaction(tx_id, only_op_id);
            }
            KeyCode::Char('s') | KeyCode::Char('S') if app.lang == Lang::Es => {
                let (tx_id, only_op_id) = (*tx_id, *only_op_id);
                app.modal = Modal::None;
                app.trigger_revert_transaction(tx_id, only_op_id);
            }
            _ => {}
        },
        Modal::SyncControlMenu {
            scope,
            selected_idx,
        } => match key.code {
            KeyCode::Esc => app.modal = Modal::None,
            KeyCode::Up | KeyCode::Char('k') => {
                *selected_idx = selected_idx.checked_sub(1).unwrap_or(2);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                *selected_idx = (*selected_idx + 1) % 3;
            }
            KeyCode::Char('1') => {
                let scope = *scope;
                app.modal = Modal::None;
                apply_sync_menu_choice(app, scope, 0);
            }
            KeyCode::Char('2') => {
                let scope = *scope;
                app.modal = Modal::None;
                apply_sync_menu_choice(app, scope, 1);
            }
            KeyCode::Char('3') => {
                let scope = *scope;
                app.modal = Modal::None;
                apply_sync_menu_choice(app, scope, 2);
            }
            KeyCode::Enter => {
                let (scope, choice) = (*scope, *selected_idx);
                app.modal = Modal::None;
                apply_sync_menu_choice(app, scope, choice);
            }
            _ => {}
        },
        Modal::ConfirmDestructiveClone { typed_confirm, .. } => match key.code {
            KeyCode::Esc => app.modal = Modal::None,
            KeyCode::Backspace => {
                typed_confirm.pop();
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                typed_confirm.push(c);
            }
            KeyCode::Enter => {
                let confirmed = typed_confirm.trim().eq_ignore_ascii_case("BORRAR")
                    || typed_confirm.trim().eq_ignore_ascii_case("DELETE");
                if confirmed {
                    app.modal = Modal::None;
                    app.trigger_plan_clone();
                } else {
                    app.status_toast = Some((
                        app.lang
                            .tr(
                                "Debes escribir exactamente BORRAR para confirmar el clonado destructivo",
                                "You must type DELETE to confirm destructive cloning",
                            )
                            .to_string(),
                        true,
                    ));
                }
            }
            _ => {}
        },
    }
}

fn queue_reset_folder_inheritance(
    app: &mut App,
    library_title: String,
    relative_folder_path: String,
) {
    if let Some(ref site) = app.current_site {
        app.staging.push(StagingOp::ResetFolderInheritance {
            site_url: site.url.clone(),
            library_title,
            relative_folder_path: relative_folder_path.clone(),
        });
        app.status_toast = Some((
            match app.lang {
                Lang::Es => format!(
                    "✓ Encolada restauración de herencia en '{}'. Pulsa 'A' para Aplicar.",
                    relative_folder_path
                ),
                Lang::En => format!(
                    "✓ Queued inheritance reset on '{}'. Press 'A' to Apply.",
                    relative_folder_path
                ),
            },
            false,
        ));
    }
}

fn submit_text_input_modal(app: &mut App, trimmed: String, kind: TextInputKind) {
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
                        match app.lang {
                            Lang::Es => format!(
                                "Encolada creación del grupo '{}'. Pulsa 'A' para Aplicar.",
                                trimmed
                            ),
                            Lang::En => format!(
                                "Queued creation of group '{}'. Press 'A' to Apply.",
                                trimmed
                            ),
                        },
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
                        match app.lang {
                            Lang::Es => format!(
                                "Encolada creación de biblioteca '{}'. Pulsa 'A' para Aplicar.",
                                trimmed
                            ),
                            Lang::En => format!(
                                "Queued creation of library '{}'. Press 'A' to Apply.",
                                trimmed
                            ),
                        },
                        false,
                    ));
                }
            }
        }
        TextInputKind::CreateFolderRelativePath => {
            let clean_input = trimmed.trim_matches('/').to_string();
            if !clean_input.is_empty() {
                if let Some(ref site) = app.current_site {
                    let lib_title = app
                        .selected_library()
                        .map(|l| l.title.clone())
                        .unwrap_or_else(|| "Documents".to_string());
                    let full_rel_path =
                        if trimmed.starts_with('/') || app.current_folder_path.is_empty() {
                            clean_input
                        } else {
                            format!(
                                "{}/{}",
                                app.current_folder_path.trim_matches('/'),
                                clean_input
                            )
                        };
                    app.staging.push(StagingOp::CreateFolder {
                        site_url: site.url.clone(),
                        library_title: lib_title.clone(),
                        relative_folder_path: full_rel_path.clone(),
                    });
                    app.status_toast = Some((
                        match app.lang {
                            Lang::Es => format!(
                                "Encolada creación de carpeta '{}/{}'. Pulsa 'A' para Aplicar.",
                                lib_title, full_rel_path
                            ),
                            Lang::En => format!(
                                "Queued creation of folder '{}/{}'. Press 'A' to Apply.",
                                lib_title, full_rel_path
                            ),
                        },
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

fn apply_sync_menu_choice(app: &mut App, scope: SyncMenuScope, choice: usize) {
    match (scope, choice) {
        (SyncMenuScope::SelectedLibrary, 0) => app.toggle_selected_library_offline_sync(),
        (SyncMenuScope::SelectedLibrary, 1) => app.toggle_selected_library_onedrive_shortcuts(),
        (SyncMenuScope::SelectedLibrary, _) => app.toggle_selected_library_sync_and_shortcuts(),
        (SyncMenuScope::SiteAndLibraries, 0) => app.toggle_site_and_libraries_offline_sync(),
        (SyncMenuScope::SiteAndLibraries, 1) => app.toggle_site_and_libraries_onedrive_shortcuts(),
        (SyncMenuScope::SiteAndLibraries, _) => app.toggle_site_and_libraries_sync_and_shortcuts(),
    }
}
