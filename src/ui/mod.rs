mod modals;
mod screens;
mod widgets;

pub use widgets::sync_scroll_offset;

use crate::app::{App, Modal, Screen, WorkspaceTab};
use crate::i18n::Lang;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

pub fn draw_ui(f: &mut Frame<'_>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Top header bar
            Constraint::Min(10),   // Main content area
            Constraint::Length(3), // Bottom status & keybindings bar
        ])
        .split(f.area());

    draw_header(f, app, chunks[0]);

    match app.screen {
        Screen::Auth => screens::draw_auth_screen(f, app, chunks[1]),
        Screen::SiteSelector => screens::draw_site_selector(f, app, chunks[1]),
        Screen::SiteWorkspace => screens::draw_site_workspace(f, app, chunks[1]),
    }

    draw_footer(f, app, chunks[2]);

    if !matches!(app.modal, Modal::None) {
        modals::draw_modal(f, app);
    }
}

fn draw_header(f: &mut Frame<'_>, app: &App, area: Rect) {
    let lang = app.lang;
    let pending_cnt = app.staging.len();
    let staging_span = if pending_cnt > 0 {
        Span::styled(
            match lang {
                Lang::Es => format!(
                    " ● [Staging: {} cambio(s) pendiente(s) — Pulsa 'A' para Aplicar] ",
                    pending_cnt
                ),
                Lang::En => format!(
                    " ● [Staging: {} pending change(s) — Press 'A' to Apply] ",
                    pending_cnt
                ),
            },
            Style::default()
                .fg(Color::Black)
                .bg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            lang.tr(" [Staging: 0 cambios] ", " [Staging: 0 changes] "),
            Style::default().fg(Color::DarkGray),
        )
    };

    let user_label = if app.user_upn.is_empty() {
        lang.tr("No autenticado", "Not signed in").to_string()
    } else {
        format!("{} ({})", app.user_display_name, app.user_upn)
    };

    let site_context = match &app.current_site {
        Some(s) => format!(" ▸ {} {} ({})", s.role.badge(), s.title, s.url),
        None => String::new(),
    };

    let site_sync_span = match app
        .current_site
        .as_ref()
        .and_then(|s| app.staging.site_sync_staging_state(&s.url))
    {
        Some(true) => Span::styled(
            lang.tr(" [Sync: BLOQUEAR*] ", " [Sync: BLOCK*] "),
            Style::default()
                .fg(Color::Black)
                .bg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
        ),
        Some(false) => Span::styled(
            lang.tr(" [Sync: ACTIVAR*] ", " [Sync: ALLOW*] "),
            Style::default()
                .fg(Color::Black)
                .bg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        ),
        None => match app.site_exclude_from_offline_client {
            Some(true) => Span::styled(
                lang.tr(" [Sync: BLOQUEADA] ", " [Sync: BLOCKED] "),
                Style::default()
                    .fg(Color::LightRed)
                    .add_modifier(Modifier::BOLD),
            ),
            Some(false) => Span::styled(
                lang.tr(" [Sync: ACTIVA] ", " [Sync: ACTIVE] "),
                Style::default().fg(Color::LightGreen),
            ),
            None => Span::raw(""),
        },
    };

    let line = Line::from(vec![
        Span::styled(
            " SPOIberia ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            match lang {
                Lang::Es => format!(" Usuario: {} ", user_label),
                Lang::En => format!(" User: {} ", user_label),
            },
            Style::default().fg(Color::White),
        ),
        Span::styled(site_context, Style::default().fg(Color::Yellow)),
        site_sync_span,
        Span::raw("  "),
        staging_span,
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan));
    f.render_widget(Paragraph::new(line).block(block), area);
}

fn draw_footer(f: &mut Frame<'_>, app: &App, area: Rect) {
    let status_line = if let Some(ref busy) = app.busy_message {
        Line::from(vec![
            Span::styled(
                app.lang.tr(" ⏳ PROCESANDO: ", " ⏳ PROCESSING: "),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {} ", busy), Style::default().fg(Color::Yellow)),
        ])
    } else if let Some((ref msg, is_err)) = app.status_toast {
        let color = if is_err {
            Color::LightRed
        } else {
            Color::LightGreen
        };
        Line::from(vec![
            Span::styled(
                format!(" {} ", msg),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" │ "),
            Span::styled(context_help_keys(app), Style::default().fg(Color::Gray)),
        ])
    } else {
        Line::from(Span::styled(
            format!(" {}", context_help_keys(app)),
            Style::default().fg(Color::Gray),
        ))
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray));
    f.render_widget(Paragraph::new(status_line).block(block), area);
}

fn context_help_keys(app: &App) -> &'static str {
    match (app.screen, app.lang) {
        (Screen::Auth, Lang::Es) => {
            "Escribe tu correo M365 | Enter: Login Navegador | F2 / Ctrl+D: Device Code | Esc: Salir"
        }
        (Screen::Auth, Lang::En) => {
            "Type your M365 email | Enter: Browser Sign-In | F2 / Ctrl+D: Device Code | Esc: Quit"
        }
        (Screen::SiteSelector, Lang::Es) => {
            "↑/↓: Mover | Enter: Abrir Sitio | /: Filtrar | s: Buscar en Tenant | u: URL Manual | c: Fijar Origen Clon | r: Recargar | ?: Ayuda | q: Salir"
        }
        (Screen::SiteSelector, Lang::En) => {
            "↑/↓: Move | Enter: Open Site | /: Filter | s: Search Tenant | u: Manual URL | c: Set Clone Source | r: Reload | ?: Help | q: Quit"
        }
        (Screen::SiteWorkspace, Lang::Es) => match app.tab {
            WorkspaceTab::GroupsAndUsers => {
                "Tab/←/→: Cambiar Panel | a: Asignar Usuario | d: Remover Usuario | g: Crear Grupo | O: Menú Sync/OneDrive Sitio | A: APLICAR Staging | H: Historial/Rollback | Esc: Sitios"
            }
            WorkspaceTab::LibrariesAndFolders => {
                "Tab/←/→: Panel | Enter: Abrir | Backspace/u: Subir | n: Nueva | p: Permiso | d: Revocar | x: Herencia | o/O: Sync | A: APLICAR"
            }
            WorkspaceTab::CloneSite => {
                "↑/↓: Mover Opción | Espacio: Activar/Desactivar | s: Cambiar URL Origen | Enter: Generar Plan en Staging | A: APLICAR | Esc: Sitios"
            }
            WorkspaceTab::StagingAndHistory => {
                "Tab/←/→: Staging vs Historial | A: APLICAR Staging | R: Revertir Transacción | r: Revertir Op | d: Quitar de Staging | X: Vaciar Staging"
            }
        },
        (Screen::SiteWorkspace, Lang::En) => match app.tab {
            WorkspaceTab::GroupsAndUsers => {
                "Tab/←/→: Switch Pane | a: Add User | d: Remove User | g: New Group | O: Site Sync/OneDrive Menu | A: APPLY Staging | H: History/Rollback | Esc: Sites"
            }
            WorkspaceTab::LibrariesAndFolders => {
                "Tab/←/→: Pane | Enter: Open | Backspace/u: Up | n: Folder | p: Grant | d: Revoke | x: Inherit | o/O: Sync | A: APPLY"
            }
            WorkspaceTab::CloneSite => {
                "↑/↓: Move Option | Space: Toggle | s: Set Source URL | Enter: Generate Staging Plan | A: APPLY | Esc: Sites"
            }
            WorkspaceTab::StagingAndHistory => {
                "Tab/←/→: Staging vs History | A: APPLY Staging | R: Revert Transaction | r: Revert Op | d: Remove Item | X: Clear Staging"
            }
        },
    }
}

pub(super) fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::graph::GraphClient;
    use crate::api::spo::SpoClient;
    use crate::app::{App, PaneFocus, Screen, SyncMenuScope};
    use crate::auth::AuthManager;
    use crate::db::Database;
    use crate::models::{FolderPermissionEntry, SiteRole, SpoSite};
    use ratatui::{backend::TestBackend, Terminal};

    fn make_test_app() -> App {
        let tmp_cfg = std::env::temp_dir().join(format!(
            "spoiberia-ui-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp_cfg);
        std::env::set_var("SPOIBERIA_CONFIG_DIR", &tmp_cfg);

        let auth = AuthManager::new().unwrap();
        let db = Database::open_in_memory().unwrap();
        let graph = GraphClient::new(auth.clone()).unwrap();
        let spo = SpoClient::new(auth.clone()).unwrap();
        let app = App::new(auth, db, graph, spo, Lang::Es);
        let _ = std::fs::remove_dir_all(&tmp_cfg);
        app
    }

    fn buffer_text(buf: &ratatui::buffer::Buffer) -> String {
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn right_column_symbols(buf: &ratatui::buffer::Buffer) -> String {
        let x = buf.area.width.saturating_sub(1);
        (0..buf.area.height)
            .map(|y| buf[(x, y)].symbol())
            .collect::<Vec<_>>()
            .join("")
    }

    #[test]
    fn test_dynamic_scrollbar_on_overflow_and_resize() {
        let mut app = make_test_app();
        app.screen = Screen::SiteSelector;
        app.sites = (0..20)
            .map(|i| SpoSite {
                title: format!("Site {:02}", i),
                url: format!("https://contoso.sharepoint.com/sites/site{:02}", i),
                description: None,
                role: SiteRole::SiteAdmin,
                is_site_admin: true,
            })
            .collect();

        let backend = TestBackend::new(80, 16);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|f| draw_ui(f, &app)).unwrap();
        let right_col = right_column_symbols(terminal.backend().buffer());
        assert!(right_col.contains('▲'), "expected top scrollbar arrow in {right_col:?}");
        assert!(right_col.contains('▼'), "expected bottom scrollbar arrow in {right_col:?}");
        assert!(right_col.contains('█'), "expected scrollbar thumb in {right_col:?}");

        app.selected_site_idx = 18;
        terminal.draw(|f| draw_ui(f, &app)).unwrap();
        assert_eq!(app.scroll.sites.get(), 13);
        let rendered = buffer_text(terminal.backend().buffer());
        assert!(rendered.contains("Site 13"));
        assert!(rendered.contains("Site 18"));
        assert!(!rendered.contains("Site 00"));
        assert!(!rendered.contains("Site 19"));

        terminal.backend_mut().resize(80, 35);
        terminal.draw(|f| draw_ui(f, &app)).unwrap();
        assert_eq!(app.scroll.sites.get(), 0);
        let right_col_tall = right_column_symbols(terminal.backend().buffer());
        assert!(!right_col_tall.contains('▲'));
        assert!(!right_col_tall.contains('▼'));
        let rendered_tall = buffer_text(terminal.backend().buffer());
        assert!(rendered_tall.contains("Site 00"));
        assert!(rendered_tall.contains("Site 19"));

        terminal.backend_mut().resize(80, 16);
        terminal.draw(|f| draw_ui(f, &app)).unwrap();
        let right_col_small_again = right_column_symbols(terminal.backend().buffer());
        assert!(right_col_small_again.contains('▲'));
        assert!(right_col_small_again.contains('▼'));
        assert_eq!(app.scroll.sites.get(), 13);
    }

    #[test]
    fn test_workspace_renders_offline_sync_badges() {
        use crate::models::{SpoLibrary, StagingOp};

        let mut app = make_test_app();
        let site_url = "https://contoso.sharepoint.com/sites/Obra1".to_string();
        app.screen = Screen::SiteWorkspace;
        app.tab = WorkspaceTab::LibrariesAndFolders;
        app.current_site = Some(SpoSite {
            title: "Obra 1".to_string(),
            url: site_url.clone(),
            description: None,
            role: SiteRole::SiteAdmin,
            is_site_admin: true,
        });
        app.site_exclude_from_offline_client = Some(true);
        app.libraries = vec![
            SpoLibrary {
                id: "lib-1".to_string(),
                title: "Docs".to_string(),
                server_relative_url: "/sites/Obra1/Shared Documents".to_string(),
                item_count: 42,
                is_default_documents: true,
                exclude_from_offline_client: true,
                views_total: 1,
                views_hiding_shortcut: 1,
                views_hiding_sync: 0,
                shortcut_button_hidden: true,
            },
            SpoLibrary {
                id: "lib-2".to_string(),
                title: "Archivos".to_string(),
                server_relative_url: "/sites/Obra1/Archivos".to_string(),
                item_count: 5,
                is_default_documents: false,
                exclude_from_offline_client: false,
                views_total: 1,
                views_hiding_shortcut: 0,
                views_hiding_sync: 0,
                shortcut_button_hidden: false,
            },
        ];

        let backend = TestBackend::new(140, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|f| draw_ui(f, &app)).unwrap();
        let rendered = buffer_text(terminal.backend().buffer());
        assert!(rendered.contains("[Sync: BLOQUEADA]"));
        assert!(rendered.contains("[NoSync]"));
        assert!(rendered.contains("[NoOD]"));
        assert!(rendered.contains("[Sync]"));
        assert!(rendered.contains("[OD]"));

        app.site_exclude_from_offline_client = Some(false);
        app.staging.push(StagingOp::SetLibraryOfflineSync {
            site_url: site_url.clone(),
            library_id: "lib-2".to_string(),
            library_title: "Archivos".to_string(),
            exclude_from_offline_client: true,
            apply_view_formatting: false,
        });
        app.staging.push(StagingOp::SetLibraryOneDriveShortcuts {
            site_url,
            library_id: "lib-2".to_string(),
            library_title: "Archivos".to_string(),
            hide_shortcut_button: true,
        });

        terminal.draw(|f| draw_ui(f, &app)).unwrap();
        let rendered_staged = buffer_text(terminal.backend().buffer());
        assert!(rendered_staged.contains("[Sync: ACTIVA]"));
        assert!(rendered_staged.contains("[-Sync*]"));
        assert!(rendered_staged.contains("[-OD*]"));

        app.open_sync_menu(SyncMenuScope::SelectedLibrary);
        terminal.draw(|f| draw_ui(f, &app)).unwrap();
        let rendered_modal = buffer_text(terminal.backend().buffer());
        assert!(rendered_modal.contains("[1] Sincronización SharePoint"));
        assert!(rendered_modal.contains("[2] Accesos directos a OneDrive"));
        assert!(rendered_modal.contains("[3] Ambos"));
    }

    #[test]
    fn test_destructive_clone_ui_warnings_and_modal() {
        use crate::models::StagingOp;

        let mut app = make_test_app();
        let site_url = "https://contoso.sharepoint.com/sites/Destino".to_string();
        app.screen = Screen::SiteWorkspace;
        app.tab = WorkspaceTab::CloneSite;
        app.current_site = Some(SpoSite {
            title: "Destino".to_string(),
            url: site_url.clone(),
            description: None,
            role: SiteRole::SiteAdmin,
            is_site_admin: true,
        });
        app.clone_source_url = "https://contoso.sharepoint.com/sites/Origen".to_string();
        app.clone_destructive = true;

        let backend = TestBackend::new(140, 28);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|f| draw_ui(f, &app)).unwrap();
        let rendered_tab = buffer_text(terminal.backend().buffer());
        assert!(rendered_tab.contains("MODO DESTRUCTIVO ACTIVADO"));

        app.modal = Modal::ConfirmDestructiveClone {
            source_url: app.clone_source_url.clone(),
            target_url: site_url.clone(),
            typed_confirm: "BOR".to_string(),
        };
        terminal.draw(|f| draw_ui(f, &app)).unwrap();
        let rendered_confirm = buffer_text(terminal.backend().buffer());
        assert!(rendered_confirm.contains("CONFIRMAR CLONADO DESTRUCTIVO"));
        assert!(rendered_confirm.contains("Escribe BORRAR"));

        app.staging.push(StagingOp::DeleteLibrary {
            site_url,
            library_id: "lib-99".to_string(),
            library_title: "Antigua".to_string(),
        });
        app.modal = Modal::ConfirmApplyStaging {
            note: String::new(),
        };
        terminal.draw(|f| draw_ui(f, &app)).unwrap();
        let rendered_apply = buffer_text(terminal.backend().buffer());
        assert!(rendered_apply.contains("CONFIRMAR APLICACIÓN DESTRUCTIVA"));
    }

    #[test]
    fn test_english_tui_rendering() {
        let mut app = make_test_app();
        app.lang = Lang::En;
        app.screen = Screen::Auth;

        let backend = TestBackend::new(140, 28);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|f| draw_ui(f, &app)).unwrap();
        let auth_txt = buffer_text(terminal.backend().buffer());
        assert!(auth_txt.contains("Microsoft 365 Authentication"));

        app.screen = Screen::SiteWorkspace;
        app.tab = WorkspaceTab::CloneSite;
        app.current_site = Some(SpoSite {
            title: "Target".to_string(),
            url: "https://contoso.sharepoint.com/sites/Target".to_string(),
            description: None,
            role: SiteRole::SiteAdmin,
            is_site_admin: true,
        });
        app.clone_destructive = true;
        terminal.draw(|f| draw_ui(f, &app)).unwrap();
        let clone_txt = buffer_text(terminal.backend().buffer());
        assert!(clone_txt.contains("DESTRUCTIVE MODE ENABLED"));

        app.modal = Modal::ConfirmDestructiveClone {
            source_url: "https://contoso.sharepoint.com/sites/Source".to_string(),
            target_url: "https://contoso.sharepoint.com/sites/Target".to_string(),
            typed_confirm: String::new(),
        };
        terminal.draw(|f| draw_ui(f, &app)).unwrap();
        let modal_txt = buffer_text(terminal.backend().buffer());
        assert!(modal_txt.contains("Type DELETE to generate the Staging plan"));
    }

    #[test]
    fn test_nested_folder_navigation_and_inheritance_modals() {
        use crate::models::{
            FolderAncestorInfo, FolderInheritanceSummary, SpoFolder, SpoLibrary,
        };

        let mut app = make_test_app();
        app.screen = Screen::SiteWorkspace;
        app.tab = WorkspaceTab::LibrariesAndFolders;
        app.focus = PaneFocus::Right;
        let site_url = "https://contoso.sharepoint.com/sites/Obra1".to_string();
        app.current_site = Some(SpoSite {
            title: "Obra 1".to_string(),
            url: site_url,
            description: None,
            role: SiteRole::SiteAdmin,
            is_site_admin: true,
        });
        app.libraries = vec![SpoLibrary {
            id: "lib-1".to_string(),
            title: "Documentos".to_string(),
            server_relative_url: "/sites/Obra1/Shared Documents".to_string(),
            item_count: 10,
            is_default_documents: true,
            exclude_from_offline_client: false,
            views_total: 1,
            views_hiding_shortcut: 0,
            views_hiding_sync: 0,
            shortcut_button_hidden: false,
        }];

        let owners = FolderPermissionEntry {
            principal_id: 3,
            principal_title: "Owners".to_string(),
            principal_login_name: "Owners".to_string(),
            principal_type: "SharePointGroup".to_string(),
            roles: vec!["Full Control".to_string()],
        };
        let members = FolderPermissionEntry {
            principal_id: 5,
            principal_title: "Members".to_string(),
            principal_login_name: "Members".to_string(),
            principal_type: "SharePointGroup".to_string(),
            roles: vec!["Edit".to_string()],
        };

        app.current_folder_path = "01_Dir/Sub".to_string();
        app.folder_ancestors = vec![
            FolderAncestorInfo {
                relative_path: String::new(),
                name: "Documentos".to_string(),
                server_relative_url: "/sites/Obra1/Shared Documents".to_string(),
                has_unique_role_assignments: false,
                role_assignments: vec![owners.clone(), members.clone()],
            },
            FolderAncestorInfo {
                relative_path: "01_Dir".to_string(),
                name: "01_Dir".to_string(),
                server_relative_url: "/sites/Obra1/Shared Documents/01_Dir".to_string(),
                has_unique_role_assignments: true,
                role_assignments: vec![owners.clone(), members.clone()],
            },
            FolderAncestorInfo {
                relative_path: "01_Dir/Sub".to_string(),
                name: "Sub".to_string(),
                server_relative_url: "/sites/Obra1/Shared Documents/01_Dir/Sub".to_string(),
                has_unique_role_assignments: false,
                role_assignments: vec![owners, members],
            },
        ];
        app.folders = vec![SpoFolder {
            name: "Deep".to_string(),
            server_relative_url: "/sites/Obra1/Shared Documents/01_Dir/Sub/Deep".to_string(),
            relative_path: "01_Dir/Sub/Deep".to_string(),
            item_id: Some(99),
            item_count: 3,
            has_unique_role_assignments: false,
            role_assignments: Vec::new(),
        }];
        app.selected_folder_idx = 1;
        assert_eq!(app.folder_list_row_count(), 2);
        assert_eq!(app.selected_folder().unwrap().name, "Deep");

        let backend = TestBackend::new(140, 32);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|f| draw_ui(f, &app)).unwrap();
        let tree_txt = buffer_text(terminal.backend().buffer());
        assert!(tree_txt.contains("[Nivel 2]"));
        assert!(tree_txt.contains("Documentos / 01_Dir / Sub"));
        assert!(tree_txt.contains("Hereda de: 01_Dir"));
        assert!(tree_txt.contains(".."));
        assert!(tree_txt.contains("Deep (3)"));

        let inh = FolderInheritanceSummary::from_ancestors(&app.folder_ancestors, Lang::Es);
        app.modal = Modal::GrantFolderPermission {
            library_title: "Documentos".to_string(),
            relative_folder_path: "01_Dir/Sub/Deep".to_string(),
            target_has_unique_roles: false,
            target_current_roles: Vec::new(),
            inheritance: inh.clone(),
            group_name: "Members".to_string(),
            role_idx: 0,
            copy_parent_roles: true,
            replace_inherited_roles: true,
            clear_subscopes: true,
            create_group_if_missing: true,
            active_field: 0,
        };
        terminal.draw(|f| draw_ui(f, &app)).unwrap();
        let grant_txt = buffer_text(terminal.backend().buffer());
        assert!(grant_txt.contains("Impacto en herencias"));
        assert!(grant_txt.contains("se revocará antes de asignar [Read]"));
        assert!(grant_txt.contains("ClearSubscopes=Sí borrará"));

        app.lang = Lang::En;
        app.modal = Modal::ManageFolderPermissions {
            library_title: "Documentos".to_string(),
            relative_folder_path: "01_Dir/Sub/Deep".to_string(),
            target_has_unique_roles: false,
            inheritance: inh.clone(),
            entries: vec![(
                "Members".to_string(),
                "SharePointGroup".to_string(),
                "Edit".to_string(),
            )],
            selected_idx: 0,
        };
        terminal.draw(|f| draw_ui(f, &app)).unwrap();
        let revoke_txt = buffer_text(terminal.backend().buffer());
        assert!(revoke_txt.contains("Revoke Folder Permission"));
        assert!(revoke_txt.contains("Members (SharePointGroup)"));

        app.modal = Modal::ConfirmResetFolderInheritance {
            library_title: "Documentos".to_string(),
            relative_folder_path: "01_Dir/Sub".to_string(),
            inheritance: inh,
        };
        terminal.draw(|f| draw_ui(f, &app)).unwrap();
        let reset_txt = buffer_text(terminal.backend().buffer());
        assert!(reset_txt.contains("Reset Folder Inheritance"));
        assert!(reset_txt.contains("Will resume inheriting permissions from: 01_Dir"));
    }
}
