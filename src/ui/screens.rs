use super::widgets::{render_scrollable_list, render_scrollable_table};
use crate::app::{App, PaneFocus, WorkspaceTab};
use crate::i18n::Lang;
use crate::models::{FolderPermissionEntry, GroupKind, SiteRole};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Row, Table, Tabs, Wrap},
    Frame,
};

pub(super) fn draw_auth_screen(f: &mut Frame<'_>, app: &App, area: Rect) {
    let lang = app.lang;
    let block = Block::default()
        .title(lang.tr(
            " Autenticación Microsoft 365 (Entra ID OAuth2) ",
            " Microsoft 365 Authentication (Entra ID OAuth2) ",
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan));

    let email_display = if app.login_email_input.is_empty() {
        Span::styled(
            lang.tr("usuario@dominio.com█", "user@domain.com█"),
            Style::default().fg(Color::DarkGray),
        )
    } else {
        Span::styled(
            format!("{}█", app.login_email_input),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
    };

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            lang.tr(
                "Bienvenido a SPOIberia — Gestión de Autorizaciones, Carpetas y Clonado en SharePoint Online",
                "Welcome to SPOIberia — SharePoint Online Permissions, Provisioning & Site Cloning",
            ),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(lang.tr(
            "Introduce tu correo corporativo M365 (para detectar automáticamente el tenant):",
            "Enter your M365 corporate email (to auto-detect your tenant):",
        )),
        Line::from(vec![
            Span::styled(
                lang.tr("  Correo / Dominio / Tenant: [ ", "  Email / Domain / Tenant: [ "),
                Style::default().fg(Color::Cyan),
            ),
            email_display,
            Span::styled(" ]", Style::default().fg(Color::Cyan)),
        ]),
        Line::from(""),
        Line::from(lang.tr(
            "Selecciona un método de inicio de sesión nativo contra Microsoft Entra ID:",
            "Choose a native Microsoft Entra ID sign-in method:",
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  [    Enter    ] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw(lang.tr(
                "Iniciar sesión interactivo en el navegador (OAuth2 + PKCE)",
                "Interactive browser sign-in (OAuth2 + PKCE)",
            )),
        ]),
        Line::from(vec![
            Span::styled("  [ F2 / Ctrl+D ] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(lang.tr(
                "Iniciar sesión mediante Código de Dispositivo (Device Code Flow)",
                "Sign in using Device Code Flow",
            )),
        ]),
        Line::from(vec![
            Span::styled("  [     Esc     ] ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw(lang.tr("Salir", "Quit")),
        ]),
    ];

    if let Some(ref prompt) = app.device_code_prompt {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            lang.tr(
                "─── CÓDIGO DE DISPOSITIVO ACTIVO ───",
                "─── ACTIVE DEVICE CODE ───",
            ),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(match lang {
            Lang::Es => format!("1. Abre en tu navegador: {}", prompt.verification_uri),
            Lang::En => format!("1. Open in your browser: {}", prompt.verification_uri),
        }));
        lines.push(Line::from(vec![
            Span::raw(lang.tr("2. Introduce el código:  ", "2. Enter the code:     ")),
            Span::styled(
                &prompt.user_code,
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    let p = Paragraph::new(lines)
        .alignment(Alignment::Center)
        .block(block)
        .wrap(Wrap { trim: true });
    f.render_widget(p, area);
}

pub(super) fn draw_site_selector(f: &mut Frame<'_>, app: &App, area: Rect) {
    let lang = app.lang;
    let filtered = app.filtered_sites();
    let filter_info = if app.site_filter.is_empty() {
        String::new()
    } else {
        match lang {
            Lang::Es => format!(" [Filtro: '{}']", app.site_filter),
            Lang::En => format!(" [Filter: '{}']", app.site_filter),
        }
    };
    let clone_src_info = if app.clone_source_url.is_empty() {
        String::new()
    } else {
        match lang {
            Lang::Es => format!(" [Origen Clonado: {}]", app.clone_source_url),
            Lang::En => format!(" [Clone Source: {}]", app.clone_source_url),
        }
    };

    let title = match lang {
        Lang::Es => format!(
            " Sitios SharePoint donde eres Administrador o Propietario ({} sitios){}{} ",
            filtered.len(),
            filter_info,
            clone_src_info
        ),
        Lang::En => format!(
            " SharePoint Sites where you are Admin or Owner ({} sites){}{} ",
            filtered.len(),
            filter_info,
            clone_src_info
        ),
    };

    let header = Row::new(vec![
        lang.tr("Rol", "Role"),
        lang.tr("Título del Sitio", "Site Title"),
        lang.tr("URL de SharePoint Online", "SharePoint Online URL"),
    ])
    .style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )
    .bottom_margin(1);

    let total_rows = filtered.len();
    let rows: Vec<Row<'_>> = filtered
        .iter()
        .enumerate()
        .map(|(idx, s)| {
            let role_color = match s.role {
                SiteRole::AdminAndOwner | SiteRole::SiteAdmin => Color::LightMagenta,
                SiteRole::Owner => Color::LightGreen,
                SiteRole::M365GroupOwner => Color::LightBlue,
            };
            let is_selected = idx == app.selected_site_idx;
            let row_style = if is_selected {
                Style::default()
                    .bg(Color::Rgb(30, 58, 95))
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            Row::new(vec![
                s.role.badge().to_string(),
                s.title.clone(),
                s.url.clone(),
            ])
            .style(row_style.fg(if is_selected { Color::White } else { role_color }))
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(15),
            Constraint::Percentage(35),
            Constraint::Percentage(50),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan)),
    );

    render_scrollable_table(
        f,
        table,
        area,
        &app.scroll.sites,
        total_rows,
        app.selected_site_idx,
        true,
    );
}

pub(super) fn draw_site_workspace(f: &mut Frame<'_>, app: &App, area: Rect) {
    let lang = app.lang;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(8)])
        .split(area);

    let tab_titles = match lang {
        Lang::Es => vec![
            format!(" [1] Grupos y Usuarios ({}) ", app.groups.len()),
            format!(" [2] Bibliotecas y Carpetas ({}) ", app.libraries.len()),
            " [3] Clonar Sitio SharePoint ".to_string(),
            format!(
                " [4] Cola Staging ({}) e Historial SQLite ({}) ",
                app.staging.len(),
                app.transactions.len()
            ),
        ],
        Lang::En => vec![
            format!(" [1] Groups & Users ({}) ", app.groups.len()),
            format!(" [2] Libraries & Folders ({}) ", app.libraries.len()),
            " [3] Clone SharePoint Site ".to_string(),
            format!(
                " [4] Staging Queue ({}) & SQLite History ({}) ",
                app.staging.len(),
                app.transactions.len()
            ),
        ],
    };

    let selected_tab_idx = match app.tab {
        WorkspaceTab::GroupsAndUsers => 0,
        WorkspaceTab::LibrariesAndFolders => 1,
        WorkspaceTab::CloneSite => 2,
        WorkspaceTab::StagingAndHistory => 3,
    };

    let tabs = Tabs::new(tab_titles)
        .select(selected_tab_idx)
        .style(Style::default().fg(Color::Gray))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(lang.tr(
                    " Módulos del Sitio (Teclas 1, 2, 3, 4 | Esc: Volver a Sitios) ",
                    " Site Modules (Keys 1, 2, 3, 4 | Esc: Back to Sites) ",
                )),
        );
    f.render_widget(tabs, chunks[0]);

    match app.tab {
        WorkspaceTab::GroupsAndUsers => draw_tab_groups_and_users(f, app, chunks[1]),
        WorkspaceTab::LibrariesAndFolders => draw_tab_libraries_and_folders(f, app, chunks[1]),
        WorkspaceTab::CloneSite => draw_tab_clone_site(f, app, chunks[1]),
        WorkspaceTab::StagingAndHistory => draw_tab_staging_and_history(f, app, chunks[1]),
    }
}

fn draw_tab_groups_and_users(f: &mut Frame<'_>, app: &App, area: Rect) {
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(area);

    let site_url = app
        .current_site
        .as_ref()
        .map(|s| s.url.as_str())
        .unwrap_or("");

    // Left Pane: SharePoint Groups
    let left_focused = app.focus == PaneFocus::Left;
    let group_items: Vec<ListItem<'_>> = app
        .groups
        .iter()
        .enumerate()
        .map(|(idx, g)| {
            let pending_cnt = app.staging.count_for_group(site_url, g.id);
            let pending_tag = if pending_cnt > 0 {
                format!(" (*{})", pending_cnt)
            } else {
                String::new()
            };
            let perms = if g.permission_levels.is_empty() {
                String::new()
            } else {
                format!(" [{}]", g.permission_levels.join(", "))
            };
            let badge_color = match g.kind {
                GroupKind::AssociatedOwner => Color::LightMagenta,
                GroupKind::AssociatedMember => Color::LightGreen,
                GroupKind::AssociatedVisitor => Color::LightBlue,
                GroupKind::Custom => Color::Yellow,
            };

            let is_sel = idx == app.selected_group_idx;
            let bg = if is_sel {
                Color::Rgb(30, 58, 95)
            } else {
                Color::Reset
            };

            let line = Line::from(vec![
                Span::styled(
                    format!("{:<9} ", g.kind.badge()),
                    Style::default()
                        .fg(badge_color)
                        .bg(bg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{}{}", g.title, pending_tag),
                    Style::default()
                        .fg(Color::White)
                        .bg(bg)
                        .add_modifier(if is_sel { Modifier::BOLD } else { Modifier::empty() }),
                ),
                Span::styled(perms, Style::default().fg(Color::Cyan).bg(bg)),
            ]);
            ListItem::new(line)
        })
        .collect();

    let groups_block = Block::default()
        .title(match app.lang {
            Lang::Es => format!(
                " Grupos del Sitio ({}) [g: Nuevo Grupo] ",
                app.groups.len()
            ),
            Lang::En => format!(
                " Site Groups ({}) [g: New Group] ",
                app.groups.len()
            ),
        })
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if left_focused {
            Color::Cyan
        } else {
            Color::DarkGray
        }));
    render_scrollable_list(
        f,
        List::new(group_items).block(groups_block),
        panes[0],
        &app.scroll.groups,
        app.groups.len(),
        app.selected_group_idx,
        left_focused,
    );

    // Right Pane: Group Members + Pending Staged Additions/Removals
    let right_focused = app.focus == PaneFocus::Right;
    let current_grp_id = app.selected_group().map(|g| g.id).unwrap_or(-1);
    let current_grp_title = app
        .selected_group()
        .map(|g| g.title.as_str())
        .unwrap_or("-");

    let mut rows: Vec<Row<'_>> = Vec::new();

    // First show pending staged additions in bright green
    let staged_adds = app.staging.pending_additions_for_group(site_url, current_grp_id);
    for (disp, ident) in &staged_adds {
        rows.push(
            Row::new(vec![
                "[+ STAGED]".to_string(),
                disp.clone(),
                ident.clone(),
                app.lang
                    .tr("Pendiente Aplicar ('A')", "Pending Apply ('A')")
                    .to_string(),
            ])
            .style(
                Style::default()
                    .fg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD),
            ),
        );
    }

    for (idx, u) in app.group_members.iter().enumerate() {
        let ident = u.clean_identifier();
        let staged_state = app
            .staging
            .user_group_staging_state(site_url, current_grp_id, &ident)
            .or_else(|| {
                app.staging
                    .user_group_staging_state(site_url, current_grp_id, &u.login_name)
            });

        let is_sel = idx == app.selected_member_idx;
        let (status_col, mut style) = match staged_state {
            Some(false) => (
                app.lang.tr("[- BAJA]", "[- REMOVE]").to_string(),
                Style::default()
                    .fg(Color::LightRed)
                    .add_modifier(Modifier::CROSSED_OUT),
            ),
            Some(true) => (
                app.lang.tr("[+ ALTA]", "[+ ADD]").to_string(),
                Style::default().fg(Color::LightGreen),
            ),
            None => (
                if u.is_site_admin {
                    "[Admin]".to_string()
                } else {
                    app.lang.tr("Activo", "Active").to_string()
                },
                Style::default().fg(Color::White),
            ),
        };

        if is_sel && right_focused {
            style = style
                .bg(Color::Rgb(30, 58, 95))
                .add_modifier(Modifier::BOLD);
        }

        rows.push(
            Row::new(vec![
                status_col,
                u.title.clone(),
                ident,
                u.id.to_string(),
            ])
            .style(style),
        );
    }

    let total_rows = rows.len();
    let sel_row = if app.group_members.is_empty() {
        0
    } else {
        staged_adds.len() + app.selected_member_idx
    };

    let members_table = Table::new(
        rows,
        [
            Constraint::Length(12),
            Constraint::Percentage(38),
            Constraint::Percentage(38),
            Constraint::Length(12),
        ],
    )
    .header(
        Row::new(vec![
            app.lang.tr("Estado", "Status"),
            app.lang.tr("Nombre", "Name"),
            "Email / UPN",
            "SP ID",
        ])
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .bottom_margin(1),
    )
    .block(
        Block::default()
            .title(match app.lang {
                Lang::Es => format!(
                    " Miembros de '{}' ({}) [a: Asignar | d: Remover | A: Aplicar] ",
                    current_grp_title,
                    app.group_members.len()
                ),
                Lang::En => format!(
                    " Members of '{}' ({}) [a: Add | d: Remove | A: Apply] ",
                    current_grp_title,
                    app.group_members.len()
                ),
            })
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(if right_focused {
                Color::Cyan
            } else {
                Color::DarkGray
            })),
    );

    render_scrollable_table(
        f,
        members_table,
        panes[1],
        &app.scroll.members,
        total_rows,
        sel_row,
        right_focused,
    );
}

fn draw_tab_libraries_and_folders(f: &mut Frame<'_>, app: &App, area: Rect) {
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(34), Constraint::Percentage(66)])
        .split(area);

    let left_focused = app.focus == PaneFocus::Left;
    let right_focused = app.focus == PaneFocus::Right;
    let site_url = app
        .current_site
        .as_ref()
        .map(|s| s.url.as_str())
        .unwrap_or("");

    let lib_items: Vec<ListItem<'_>> = app
        .libraries
        .iter()
        .enumerate()
        .map(|(idx, l)| {
            let is_sel = idx == app.selected_library_idx;
            let bg = if is_sel {
                Color::Rgb(30, 58, 95)
            } else {
                Color::Reset
            };
            let (sync_tag, sync_style) =
                match app.staging.library_sync_staging_state(site_url, &l.id) {
                    Some(true) => (
                        "[-Sync*] ",
                        Style::default()
                            .fg(Color::LightRed)
                            .bg(bg)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Some(false) => (
                        "[+Sync*] ",
                        Style::default()
                            .fg(Color::LightGreen)
                            .bg(bg)
                            .add_modifier(Modifier::BOLD),
                    ),
                    None if l.exclude_from_offline_client => {
                        ("[NoSync] ", Style::default().fg(Color::LightRed).bg(bg))
                    }
                    None => ("[Sync] ", Style::default().fg(Color::Cyan).bg(bg)),
                };
            let (od_tag, od_style) =
                match app.staging.library_shortcut_staging_state(site_url, &l.id) {
                    Some(true) => (
                        "[-OD*] ",
                        Style::default()
                            .fg(Color::LightRed)
                            .bg(bg)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Some(false) => (
                        "[+OD*] ",
                        Style::default()
                            .fg(Color::LightGreen)
                            .bg(bg)
                            .add_modifier(Modifier::BOLD),
                    ),
                    None if l.shortcut_button_hidden => {
                        ("[NoOD] ", Style::default().fg(Color::LightRed).bg(bg))
                    }
                    None => ("[OD] ", Style::default().fg(Color::Cyan).bg(bg)),
                };
            let def_tag = if l.is_default_documents {
                "[Default] "
            } else {
                ""
            };
            ListItem::new(Line::from(vec![
                Span::styled(sync_tag, sync_style),
                Span::styled(od_tag, od_style),
                Span::styled(def_tag, Style::default().fg(Color::LightGreen).bg(bg)),
                Span::styled(
                    match app.lang {
                        Lang::Es => format!("{} ({} ítems)", l.title, l.item_count),
                        Lang::En => format!("{} ({} items)", l.title, l.item_count),
                    },
                    Style::default()
                        .fg(Color::White)
                        .bg(bg)
                        .add_modifier(if is_sel { Modifier::BOLD } else { Modifier::empty() }),
                ),
            ]))
        })
        .collect();

    let lib_block = Block::default()
        .title(app.lang.tr(
            " Bibliotecas [b: Nueva | o: Sync/OneDrive] ",
            " Libraries [b: New | o: Sync/OneDrive] ",
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if left_focused {
            Color::Cyan
        } else {
            Color::DarkGray
        }));
    render_scrollable_list(
        f,
        List::new(lib_items).block(lib_block),
        panes[0],
        &app.scroll.libraries,
        app.libraries.len(),
        app.selected_library_idx,
        left_focused,
    );

    let current_lib = app
        .selected_library()
        .map(|l| l.title.as_str())
        .unwrap_or("Documentos");
    let cur_inheritance = app.current_level_inheritance();

    let format_roles = |entries: &[FolderPermissionEntry]| -> String {
        entries
            .iter()
            .map(|r| format!("{}: [{}]", r.principal_title, r.roles.join(",")))
            .collect::<Vec<_>>()
            .join(" | ")
    };

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(6)])
        .split(panes[1]);

    let depth = if app.current_folder_path.is_empty() {
        0
    } else {
        app.current_folder_path
            .split('/')
            .filter(|s| !s.is_empty())
            .count()
    };
    let breadcrumb = if app.current_folder_path.is_empty() {
        format!("📁 {}", current_lib)
    } else {
        format!(
            "📁 {} / {}",
            current_lib,
            app.current_folder_path.replace('/', " / ")
        )
    };
    let depth_badge = match (depth, app.lang) {
        (0, Lang::Es) => "[Raíz]".to_string(),
        (0, Lang::En) => "[Root]".to_string(),
        (n, Lang::Es) => format!("[Nivel {}]", n),
        (n, Lang::En) => format!("[Level {}]", n),
    };

    let current_unique_ancestor = app
        .folder_ancestors
        .last()
        .filter(|a| !a.relative_path.is_empty() && a.has_unique_role_assignments);
    let inheritance_line = if let Some(anc) = current_unique_ancestor {
        let roles_txt = format_roles(&anc.role_assignments);
        Line::from(vec![
            Span::styled(
                app.lang.tr(
                    "🔒 Permisos ÚNICOS (herencia rota) ",
                    "🔒 UNIQUE permissions (broken inheritance) ",
                ),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if roles_txt.is_empty() {
                    app.lang
                        .tr("(sin asignaciones)", "(no assignments)")
                        .to_string()
                } else {
                    roles_txt
                },
                Style::default().fg(Color::Gray),
            ),
        ])
    } else {
        let roles_txt = format_roles(&cur_inheritance.inherited_roles);
        Line::from(vec![
            Span::styled(
                match app.lang {
                    Lang::Es => format!("↳ Hereda de: {} ", cur_inheritance.inherited_from_label),
                    Lang::En => {
                        format!("↳ Inherits from: {} ", cur_inheritance.inherited_from_label)
                    }
                },
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(
                if roles_txt.is_empty() {
                    String::new()
                } else {
                    format!("— {}", roles_txt)
                },
                Style::default().fg(Color::Gray),
            ),
        ])
    };

    let path_header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                format!("{} ", depth_badge),
                Style::default()
                    .fg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                breadcrumb,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        inheritance_line,
    ])
    .block(
        Block::default()
            .title(app.lang.tr(" Ruta y Herencia Actual ", " Current Path & Inheritance "))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(if right_focused {
                Color::Cyan
            } else {
                Color::DarkGray
            })),
    );
    f.render_widget(path_header, right_chunks[0]);

    let mut rows: Vec<Row<'_>> = Vec::new();
    let parent_offset = usize::from(app.is_in_subfolder());
    if app.is_in_subfolder() {
        let is_sel = app.selected_folder_idx == 0;
        let style = if is_sel && right_focused {
            Style::default()
                .bg(Color::Rgb(30, 58, 95))
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        };
        rows.push(
            Row::new(vec![
                "📁 ..".to_string(),
                app.lang.tr("↑ Subir", "↑ Up").to_string(),
                app.lang
                    .tr(
                        "Volver al nivel superior (Enter / Backspace / u)",
                        "Go up one level (Enter / Backspace / u)",
                    )
                    .to_string(),
            ])
            .style(style),
        );
    }

    for (idx, folder) in app.folders.iter().enumerate() {
        let row_idx = idx + parent_offset;
        let is_sel = row_idx == app.selected_folder_idx;
        let perms_desc = if folder.has_unique_role_assignments {
            if folder.role_assignments.is_empty() {
                app.lang
                    .tr("Permisos únicos", "Unique permissions")
                    .to_string()
            } else {
                format_roles(&folder.role_assignments)
            }
        } else if !cur_inheritance.inherited_roles.is_empty() {
            format!("(↳) {}", format_roles(&cur_inheritance.inherited_roles))
        } else {
            match app.lang {
                Lang::Es => format!("Hereda de {}", cur_inheritance.inherited_from_label),
                Lang::En => format!("Inherits from {}", cur_inheritance.inherited_from_label),
            }
        };

        let style = if is_sel && right_focused {
            Style::default()
                .bg(Color::Rgb(30, 58, 95))
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else if folder.has_unique_role_assignments {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::White)
        };

        let inherit_col = if folder.has_unique_role_assignments {
            app.lang.tr("Únicos (Rota)", "Unique (Broken)").to_string()
        } else {
            format!("↳ {}", cur_inheritance.inherited_from_label)
        };

        let label = if folder.item_count > 0 {
            format!("📁 {} ({})", folder.name, folder.item_count)
        } else {
            format!("📁 {}", folder.name)
        };

        rows.push(Row::new(vec![label, inherit_col, perms_desc]).style(style));
    }

    let folders_table = Table::new(
        rows,
        [
            Constraint::Percentage(28),
            Constraint::Length(22),
            Constraint::Percentage(50),
        ],
    )
    .header(
        Row::new(vec![
            app.lang.tr("Carpeta", "Folder"),
            app.lang.tr("Herencia", "Inheritance"),
            app.lang
                .tr("Grupos y Permisos Efectivos", "Effective Groups & Permissions"),
        ])
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .bottom_margin(1),
    )
    .block(
        Block::default()
            .title(app.lang.tr(
                " Carpetas [Enter: Abrir | Backspace/u: Subir | n: Nueva | p: Permiso | d: Revocar | x: Herencia | i/e: CSV/JSON] ",
                " Folders [Enter: Open | Backspace/u: Up | n: New | p: Grant | d: Revoke | x: Inherit | i/e: CSV/JSON] ",
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(if right_focused {
                Color::Cyan
            } else {
                Color::DarkGray
            })),
    );

    render_scrollable_table(
        f,
        folders_table,
        right_chunks[1],
        &app.scroll.folders,
        app.folder_list_row_count(),
        app.selected_folder_idx,
        right_focused,
    );
}

fn draw_tab_clone_site(f: &mut Frame<'_>, app: &App, area: Rect) {
    let lang = app.lang;
    let target_url = app
        .current_site
        .as_ref()
        .map(|s| s.url.as_str())
        .unwrap_or("-");

    let src_display = if app.clone_source_url.is_empty() {
        lang.tr(
            "(No seleccionado — pulsa 's' para escribir URL o 'c' en el Selector de Sitios)",
            "(Not selected — press 's' to enter URL or 'c' in the Site Selector)",
        )
        .to_string()
    } else {
        app.clone_source_url.clone()
    };

    let options = match lang {
        Lang::Es => [
            format!(
                "[{}] 1. Clonar Grupos Asociados (Owners/Members/Visitors), Grupos Custom y sus Miembros",
                if app.clone_include_groups { "x" } else { " " }
            ),
            format!(
                "[{}] 2. Clonar Bibliotecas y Copiar Carpetas/Archivos Server-Side (conservando historial)",
                if app.clone_include_files { "x" } else { " " }
            ),
            format!(
                "[{}] 3. Replicar Permisos Únicos a nivel de Carpeta (Herencia Rota top-down)",
                if app.clone_include_unique_perms { "x" } else { " " }
            ),
            format!(
                "[{}] 4. Sobreescribir archivos si ya existen en el destino",
                if app.clone_overwrite { "x" } else { " " }
            ),
            if app.clone_destructive {
                "[x] 5. MODO DESTRUCTIVO: BORRAR bibliotecas, carpetas, archivos, grupos y usuarios del destino antes de clonar".to_string()
            } else {
                "[ ] 5. Modo de clonado: ADITIVO (conserva contenido y grupos previos del destino; pulsa Espacio para DESTRUCTIVO)".to_string()
            },
        ],
        Lang::En => [
            format!(
                "[{}] 1. Clone Associated Groups (Owners/Members/Visitors), Custom Groups & Members",
                if app.clone_include_groups { "x" } else { " " }
            ),
            format!(
                "[{}] 2. Clone Libraries & Server-Side Copy Folders/Files (preserving version history)",
                if app.clone_include_files { "x" } else { " " }
            ),
            format!(
                "[{}] 3. Replicate Unique Folder Permissions (Broken Inheritance top-down)",
                if app.clone_include_unique_perms { "x" } else { " " }
            ),
            format!(
                "[{}] 4. Overwrite existing files at target",
                if app.clone_overwrite { "x" } else { " " }
            ),
            if app.clone_destructive {
                "[x] 5. DESTRUCTIVE MODE: DELETE existing libraries, folders, files, groups & users at target before cloning".to_string()
            } else {
                "[ ] 5. Clone mode: ADDITIVE (keeps existing target content & groups; press Space for DESTRUCTIVE)".to_string()
            },
        ],
    };

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                lang.tr("  Sitio ORIGEN  (Source): ", "  SOURCE Site: "),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::styled(src_display, Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::styled(
                lang.tr("  Sitio DESTINO (Target): ", "  TARGET Site: "),
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            ),
            Span::styled(target_url, Style::default().fg(Color::White)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            lang.tr(
                "  Opciones de Clonado (Usa ↑/↓ y Espacio para marcar/desmarcar):",
                "  Clone Options (Use ↑/↓ and Space to toggle):",
            ),
            Style::default().fg(Color::Cyan),
        )),
        Line::from(""),
    ];

    for (idx, opt_text) in options.iter().enumerate() {
        let is_sel = idx == app.clone_selected_option;
        let is_destructive_opt = idx == 4 && app.clone_destructive;
        let style = if is_sel && is_destructive_opt {
            Style::default()
                .fg(Color::White)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD)
        } else if is_sel {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else if is_destructive_opt {
            Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(opt_text.clone(), style),
        ]));
    }

    if app.clone_destructive {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            lang.tr(
                "  ⚠⚠⚠ ATENCIÓN: MODO DESTRUCTIVO ACTIVADO ⚠⚠⚠",
                "  ⚠⚠⚠ WARNING: DESTRUCTIVE MODE ENABLED ⚠⚠⚠",
            ),
            Style::default()
                .fg(Color::White)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            lang.tr(
                "  Se ELIMINARÁN bibliotecas, carpetas, archivos, grupos y usuarios existentes en el sitio DESTINO antes de clonar.",
                "  Existing libraries, folders, files, groups and users on the TARGET site will be DELETED before cloning.",
            ),
            Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            lang.tr(
                "  Las eliminaciones destructivas NO son reversibles mediante Rollback en SQLite.",
                "  Destructive deletions are NOT reversible via SQLite Rollback.",
            ),
            Style::default().fg(Color::Yellow),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(
            "  [s] ",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ),
        Span::raw(lang.tr("Indicar URL del Sitio Origen   ", "Set Source Site URL   ")),
        Span::styled(
            "  [Enter] ",
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        ),
        Span::raw(lang.tr(
            "Generar Plan de Clonado en Cola Staging (para revisar y Aplicar con 'A')",
            "Generate Clone Plan in Staging Queue (review and Apply with 'A')",
        )),
    ]));

    let block = Block::default()
        .title(lang.tr(
            " Fase 3: Clonado Completo de un Sitio SharePoint en otro ",
            " Phase 3: Full SharePoint Site Cloning ",
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan));

    f.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: false }), area);
}

fn draw_tab_staging_and_history(f: &mut Frame<'_>, app: &App, area: Rect) {
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    let left_focused = app.focus == PaneFocus::Left;
    let right_focused = app.focus == PaneFocus::Right;

    // Left: Staging Queue (Pending operations before clicking Apply)
    let staging_items: Vec<ListItem<'_>> = if app.staging.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            app.lang.tr(
                "  (Cola de Staging vacía — añade/quita usuarios o importa un CSV/JSON)",
                "  (Staging queue empty — add/remove users or import a CSV/JSON)",
            ),
            Style::default().fg(Color::DarkGray),
        )))]
    } else {
        app.staging
            .ops
            .iter()
            .enumerate()
            .map(|(idx, op)| {
                let is_sel = idx == app.selected_staging_idx && left_focused;
                let label = op.label_for(app.lang);
                let fg = if label.starts_with("[+]")
                    || label.starts_with("[+Rol]")
                    || label.starts_with("[+Sync]")
                    || label.starts_with("[+AtajoOD]")
                {
                    Color::LightGreen
                } else if label.starts_with("[-]")
                    || label.starts_with("[-Sync]")
                    || label.starts_with("[-AtajoOD]")
                    || label.starts_with("[-DEL")
                {
                    Color::LightRed
                } else {
                    Color::Yellow
                };
                let style = if is_sel {
                    Style::default()
                        .fg(Color::White)
                        .bg(Color::Rgb(30, 58, 95))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(fg)
                };
                ListItem::new(Line::from(Span::styled(
                    format!("{:>2}. {}", idx + 1, label),
                    style,
                )))
            })
            .collect()
    };

    let staging_block = Block::default()
        .title(match app.lang {
            Lang::Es => format!(
                " Cola Staging ({}) [A: Aplicar Cambios | d: Quitar ítem | X: Vaciar] ",
                app.staging.len()
            ),
            Lang::En => format!(
                " Staging Queue ({}) [A: Apply Changes | d: Remove item | X: Clear] ",
                app.staging.len()
            ),
        })
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if left_focused {
            Color::Cyan
        } else {
            Color::DarkGray
        }));
    render_scrollable_list(
        f,
        List::new(staging_items).block(staging_block),
        panes[0],
        &app.scroll.staging,
        app.staging.len(),
        app.selected_staging_idx,
        left_focused,
    );

    // Right: SQLite Transactions History + Selected Transaction Operations
    let right_split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(52), Constraint::Percentage(48)])
        .split(panes[1]);

    let tx_rows: Vec<Row<'_>> = app
        .transactions
        .iter()
        .enumerate()
        .map(|(idx, tx)| {
            let is_sel = idx == app.selected_tx_idx;
            let status_color = match tx.status.as_str() {
                "APPLIED" => Color::LightGreen,
                "REVERTED" => Color::Yellow,
                "PARTIAL" => Color::LightCyan,
                _ => Color::LightRed,
            };
            let style = if is_sel && right_focused {
                Style::default()
                    .bg(Color::Rgb(30, 58, 95))
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(status_color)
            };
            let short_date = tx.created_at.chars().take(19).collect::<String>();
            Row::new(vec![
                format!("#{}", tx.id),
                short_date,
                tx.status.clone(),
                tx.summary.clone(),
            ])
            .style(style)
        })
        .collect();

    let tx_table = Table::new(
        tx_rows,
        [
            Constraint::Length(6),
            Constraint::Length(20),
            Constraint::Length(10),
            Constraint::Percentage(64),
        ],
    )
    .header(
        Row::new(vec![
            "ID",
            app.lang.tr("Fecha (UTC)", "Date (UTC)"),
            app.lang.tr("Estado", "Status"),
            app.lang.tr("Resumen de Acciones", "Actions Summary"),
        ])
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .bottom_margin(1),
    )
    .block(
        Block::default()
            .title(match app.lang {
                Lang::Es => format!(
                    " Historial de Transacciones SQLite ({}) [R: Revertir Transacción] ",
                    app.transactions.len()
                ),
                Lang::En => format!(
                    " SQLite Transaction History ({}) [R: Revert Transaction] ",
                    app.transactions.len()
                ),
            })
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(if right_focused {
                Color::Cyan
            } else {
                Color::DarkGray
            })),
    );
    render_scrollable_table(
        f,
        tx_table,
        right_split[0],
        &app.scroll.transactions,
        app.transactions.len(),
        app.selected_tx_idx,
        right_focused,
    );

    let op_items: Vec<ListItem<'_>> = app
        .selected_tx_ops
        .iter()
        .enumerate()
        .map(|(idx, op)| {
            let is_sel = idx == app.selected_tx_op_idx;
            let color = match op.status.as_str() {
                "APPLIED" => Color::Green,
                "REVERTED" => Color::Yellow,
                _ => Color::Red,
            };
            let prefix = if is_sel { "▸ " } else { "  " };
            ListItem::new(Line::from(Span::styled(
                format!(
                    "{}#{} (seq {}) [{}] {}",
                    prefix, op.id, op.seq_order, op.status, op.target_label
                ),
                Style::default().fg(color),
            )))
        })
        .collect();

    let ops_block = Block::default()
        .title(app.lang.tr(
            " Operaciones Atómicas de la Transacción Seleccionada [r: Revertir Operación] ",
            " Atomic Operations of Selected Transaction [r: Revert Operation] ",
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray));
    render_scrollable_list(
        f,
        List::new(op_items).block(ops_block),
        right_split[1],
        &app.scroll.tx_ops,
        app.selected_tx_ops.len(),
        app.selected_tx_op_idx,
        right_focused,
    );
}
