use crate::app::{
    App, Modal, PaneFocus, Screen, SyncMenuScope, WorkspaceTab, STANDARD_ROLES,
};
use crate::i18n::Lang;
use crate::models::{analyze_permission_change_impact, FolderPermissionEntry, GroupKind, SiteRole};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Row, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Table, TableState, Tabs, Wrap,
    },
    Frame,
};
use std::cell::Cell;

pub fn sync_scroll_offset(
    offset_cell: &Cell<usize>,
    total_items: usize,
    selected_idx: usize,
    viewport_rows: usize,
) -> (usize, bool) {
    if total_items == 0 || viewport_rows == 0 || total_items <= viewport_rows {
        offset_cell.set(0);
        return (0, false);
    }
    let selected = selected_idx.min(total_items - 1);
    let mut offset = offset_cell.get();
    if selected < offset {
        offset = selected;
    } else if selected >= offset + viewport_rows {
        offset = selected + 1 - viewport_rows;
    }
    let max_offset = total_items.saturating_sub(viewport_rows);
    offset = offset.min(max_offset);
    offset_cell.set(offset);
    (offset, true)
}

#[allow(clippy::too_many_arguments)]
fn render_vertical_scrollbar(
    f: &mut Frame<'_>,
    area: Rect,
    top_inset: u16,
    bottom_inset: u16,
    total_items: usize,
    viewport_rows: usize,
    offset: usize,
    is_focused: bool,
) {
    let height = area
        .height
        .saturating_sub(top_inset.saturating_add(bottom_inset));
    if area.width == 0 || height < 2 {
        return;
    }
    let sb_area = Rect {
        x: area.x,
        y: area.y.saturating_add(top_inset),
        width: area.width,
        height,
    };
    let color = if is_focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("▲"))
        .end_symbol(Some("▼"))
        .track_symbol(Some("│"))
        .thumb_symbol("█")
        .style(Style::default().fg(color));
    let mut sb_state = ScrollbarState::new(
        total_items.saturating_sub(viewport_rows).saturating_add(1),
    )
    .position(offset)
    .viewport_content_length(viewport_rows);
    f.render_stateful_widget(scrollbar, sb_area, &mut sb_state);
}

fn render_scrollable_list(
    f: &mut Frame<'_>,
    list: List<'_>,
    area: Rect,
    offset_cell: &Cell<usize>,
    total_items: usize,
    selected_idx: usize,
    is_focused: bool,
) {
    let viewport_rows = area.height.saturating_sub(2) as usize;
    let (offset, show_scrollbar) =
        sync_scroll_offset(offset_cell, total_items, selected_idx, viewport_rows);
    let selected = if total_items > 0 {
        Some(selected_idx.min(total_items - 1))
    } else {
        None
    };
    let mut state = ListState::default()
        .with_offset(offset)
        .with_selected(selected);
    f.render_stateful_widget(list, area, &mut state);
    if show_scrollbar {
        render_vertical_scrollbar(
            f,
            area,
            1,
            1,
            total_items,
            viewport_rows,
            offset,
            is_focused,
        );
    }
}

fn render_scrollable_table(
    f: &mut Frame<'_>,
    table: Table<'_>,
    area: Rect,
    offset_cell: &Cell<usize>,
    total_items: usize,
    selected_idx: usize,
    is_focused: bool,
) {
    let viewport_rows = area.height.saturating_sub(4) as usize;
    let (offset, show_scrollbar) =
        sync_scroll_offset(offset_cell, total_items, selected_idx, viewport_rows);
    let selected = if total_items > 0 {
        Some(selected_idx.min(total_items - 1))
    } else {
        None
    };
    let mut state = TableState::default()
        .with_offset(offset)
        .with_selected(selected);
    f.render_stateful_widget(table, area, &mut state);
    if show_scrollbar {
        render_vertical_scrollbar(
            f,
            area,
            3,
            1,
            total_items,
            viewport_rows,
            offset,
            is_focused,
        );
    }
}

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
        Screen::Auth => draw_auth_screen(f, app, chunks[1]),
        Screen::SiteSelector => draw_site_selector(f, app, chunks[1]),
        Screen::SiteWorkspace => draw_site_workspace(f, app, chunks[1]),
    }

    draw_footer(f, app, chunks[2]);

    if !matches!(app.modal, Modal::None) {
        draw_modal(f, app);
    }
}

fn draw_header(f: &mut Frame<'_>, app: &App, area: Rect) {
    let lang = app.lang;
    let pending_cnt = app.staging.len();
    let staging_span = if pending_cnt > 0 {
        Span::styled(
            match lang {
                Lang::Es => format!(" ● [Staging: {} cambio(s) pendiente(s) — Pulsa 'A' para Aplicar] ", pending_cnt),
                Lang::En => format!(" ● [Staging: {} pending change(s) — Press 'A' to Apply] ", pending_cnt),
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

fn draw_auth_screen(f: &mut Frame<'_>, app: &App, area: Rect) {
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

fn draw_site_selector(f: &mut Frame<'_>, app: &App, area: Rect) {
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

fn draw_site_workspace(f: &mut Frame<'_>, app: &App, area: Rect) {
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
                    Style::default().fg(badge_color).bg(bg).add_modifier(Modifier::BOLD),
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
                app.lang.tr("🔒 Permisos ÚNICOS (herencia rota) ", "🔒 UNIQUE permissions (broken inheritance) "),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if roles_txt.is_empty() {
                    app.lang.tr("(sin asignaciones)", "(no assignments)").to_string()
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
                    Lang::En => format!("↳ Inherits from: {} ", cur_inheritance.inherited_from_label),
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
            app.lang.tr("Grupos y Permisos Efectivos", "Effective Groups & Permissions"),
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
            Span::styled(format!(" {} ", msg), Style::default().fg(color).add_modifier(Modifier::BOLD)),
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
        (Screen::Auth, Lang::Es) => "Escribe tu correo M365 | Enter: Login Navegador | F2 / Ctrl+D: Device Code | Esc: Salir",
        (Screen::Auth, Lang::En) => "Type your M365 email | Enter: Browser Sign-In | F2 / Ctrl+D: Device Code | Esc: Quit",
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

fn draw_modal(f: &mut Frame<'_>, app: &App) {
    let lang = app.lang;
    match &app.modal {
        Modal::None => {}
        Modal::Help => {
            let area = centered_rect(76, 82, f.area());
            f.render_widget(Clear, area);
            let lines = match lang {
                Lang::Es => vec![
                    Line::from(Span::styled(
                        "Atajos de Teclado de SPOIberia",
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from("• Selector de Sitios:"),
                    Line::from("  Enter       Abrir sitio SharePoint seleccionado"),
                    Line::from("  /           Filtrar lista local de sitios"),
                    Line::from("  s           Buscar sitios en todo el tenant M365 y verificar permisos"),
                    Line::from("  u           Abrir un sitio introduciendo su URL manualmente"),
                    Line::from("  c           Marcar sitio seleccionado como Origen para Clonado (Fase 3)"),
                    Line::from(""),
                    Line::from("• Módulo 1 — Grupos y Usuarios (Pestaña 1):"),
                    Line::from("  a           Buscar usuario en el directorio M365 y encolar alta en el grupo"),
                    Line::from("  d / Supr    Encolar baja del usuario seleccionado del grupo"),
                    Line::from("  g           Crear nuevo grupo de SharePoint en el sitio"),
                    Line::from(""),
                    Line::from("• Módulo 2 — Bibliotecas, Carpetas y Sync Offline (Pestaña 2):"),
                    Line::from("  Enter       Entrar en carpeta seleccionada (o subir si '..' está seleccionado)"),
                    Line::from("  Backspace/u Subir un nivel en el árbol de carpetas"),
                    Line::from("  n / b       Crear nueva carpeta en nivel actual (n) o nueva biblioteca (b)"),
                    Line::from("  o / O       Abrir menú Sync SharePoint / Accesos OneDrive en biblioteca (o) o sitio+bibs (O)"),
                    Line::from("  p / d / x   Asignar permiso (p), revocar rol (d) o restaurar herencia (x)"),
                    Line::from("  i / e       Importar matriz de carpetas/permisos CSV/JSON (i) o exportar (e)"),
                    Line::from(""),
                    Line::from("• Staging y Transacciones SQLite (Pestaña 4 / Global):"),
                    Line::from("  A           APLICAR todos los cambios en Staging y registrar transacción en SQLite"),
                    Line::from("  R / r       Revertir (Rollback) una transacción completa (R) o una operación (r)"),
                    Line::from(""),
                    Line::from(Span::styled("Pulsa Esc o ? para cerrar esta ayuda", Style::default().fg(Color::Yellow))),
                ],
                Lang::En => vec![
                    Line::from(Span::styled(
                        "SPOIberia Keyboard Shortcuts",
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from("• Site Selector:"),
                    Line::from("  Enter       Open selected SharePoint site"),
                    Line::from("  /           Filter local site list"),
                    Line::from("  s           Search sites across M365 tenant and verify permissions"),
                    Line::from("  u           Open a site by entering its URL manually"),
                    Line::from("  c           Mark selected site as Clone Source (Phase 3)"),
                    Line::from(""),
                    Line::from("• Module 1 — Groups & Users (Tab 1):"),
                    Line::from("  a           Search user in M365 directory and queue addition to group"),
                    Line::from("  d / Del     Queue removal of selected user from group"),
                    Line::from("  g           Create new SharePoint group on site"),
                    Line::from(""),
                    Line::from("• Module 2 — Libraries, Folders & Offline Sync (Tab 2):"),
                    Line::from("  Enter       Open selected folder (or go up when '..' is selected)"),
                    Line::from("  Backspace/u Go up one level in folder tree"),
                    Line::from("  n / b       Create new folder in current level (n) or new library (b)"),
                    Line::from("  o / O       Open SharePoint Sync / OneDrive Shortcuts menu for library (o) or site+libs (O)"),
                    Line::from("  p / d / x   Grant permission (p), revoke role (d), or reset inheritance (x)"),
                    Line::from("  i / e       Import folders/permissions CSV/JSON matrix (i) or export (e)"),
                    Line::from(""),
                    Line::from("• Staging & SQLite Transactions (Tab 4 / Global):"),
                    Line::from("  A           APPLY all Staging changes and record transaction in SQLite"),
                    Line::from("  R / r       Revert (Rollback) full transaction (R) or single operation (r)"),
                    Line::from(""),
                    Line::from(Span::styled("Press Esc or ? to close this help", Style::default().fg(Color::Yellow))),
                ],
            };
            let block = Block::default()
                .title(lang.tr(" Ayuda de SPOIberia ", " SPOIberia Help "))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Yellow));
            f.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: false }), area);
        }
        Modal::TextInput {
            title,
            prompt,
            value,
            ..
        } => {
            let area = centered_rect(65, 28, f.area());
            f.render_widget(Clear, area);
            let lines = vec![
                Line::from(""),
                Line::from(Span::styled(prompt.as_str(), Style::default().fg(Color::Cyan))),
                Line::from(""),
                Line::from(Span::styled(
                    format!(" > {}█", value),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    lang.tr(
                        " [Enter] Confirmar   [Esc] Cancelar",
                        " [Enter] Confirm   [Esc] Cancel",
                    ),
                    Style::default().fg(Color::Gray),
                )),
            ];
            let block = Block::default()
                .title(format!(" {} ", title))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Yellow));
            f.render_widget(Paragraph::new(lines).block(block), area);
        }
        Modal::AddUserToGroup {
            group_title,
            query,
            results,
            selected_idx,
            marked_identifiers,
            searching,
            ..
        } => {
            let area = centered_rect(76, 70, f.area());
            f.render_widget(Clear, area);
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(4),
                    Constraint::Min(8),
                    Constraint::Length(3),
                ])
                .split(area);

            let status = if *searching {
                lang.tr(" (Buscando...)", " (Searching...)")
            } else {
                ""
            };
            let search_block = Block::default()
                .title(match lang {
                    Lang::Es => format!(
                        " Asignar Usuario(s) al Grupo '{}'{} ",
                        group_title, status
                    ),
                    Lang::En => format!(
                        " Add User(s) to Group '{}'{} ",
                        group_title, status
                    ),
                })
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::LightGreen));
            let search_line = Line::from(vec![
                Span::styled(
                    lang.tr(
                        " Buscar en directorio (o escribe email/UPN): ",
                        " Search directory (or type email/UPN): ",
                    ),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!("{}█", query),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
            ]);
            f.render_widget(Paragraph::new(search_line).block(search_block), chunks[0]);

            let items: Vec<ListItem<'_>> = results
                .iter()
                .enumerate()
                .map(|(idx, u)| {
                    let ident = u.identifier().to_string();
                    let is_marked = marked_identifiers.contains(&ident);
                    let is_sel = idx == *selected_idx;
                    let check = if is_marked { "[✓]" } else { "[ ]" };
                    let style = if is_sel {
                        Style::default()
                            .bg(Color::Rgb(30, 58, 95))
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    } else if is_marked {
                        Style::default().fg(Color::LightGreen)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    ListItem::new(Line::from(Span::styled(
                        format!(
                            " {} {:<30} <{}> ({})",
                            check,
                            u.display_name,
                            ident,
                            u.department.as_deref().unwrap_or("-")
                        ),
                        style,
                    )))
                })
                .collect();

            let list_block = Block::default()
                .title(match lang {
                    Lang::Es => format!(
                        " Resultados del Directorio ({}) — Seleccionados: {} ",
                        results.len(),
                        marked_identifiers.len()
                    ),
                    Lang::En => format!(
                        " Directory Results ({}) — Selected: {} ",
                        results.len(),
                        marked_identifiers.len()
                    ),
                })
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Cyan));
            render_scrollable_list(
                f,
                List::new(items).block(list_block),
                chunks[1],
                &app.scroll.modal_users,
                results.len(),
                *selected_idx,
                true,
            );

            let help_block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded);
            f.render_widget(
                Paragraph::new(lang.tr(
                    " [↑/↓] Mover | [Tab] Marcar varios | [Ctrl+S] Buscar ahora | [Enter] Encolar en Staging | [Esc] Cancelar",
                    " [↑/↓] Move | [Tab] Multi-select | [Ctrl+S] Search now | [Enter] Queue in Staging | [Esc] Cancel",
                ))
                .block(help_block),
                chunks[2],
            );
        }
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
        } => {
            let area = centered_rect(82, 78, f.area());
            f.render_widget(Clear, area);
            let role_name = STANDARD_ROLES.get(*role_idx).copied().unwrap_or("Read");
            let impact = analyze_permission_change_impact(
                *target_has_unique_roles,
                target_current_roles,
                inheritance,
                group_name,
                role_name,
                *copy_parent_roles,
                *clear_subscopes,
            );

            let field_style = |idx: usize| -> Style {
                if *active_field == idx {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                }
            };

            let yes = lang.tr("Sí", "Yes");
            let no = "No";
            let state_badge = if *target_has_unique_roles {
                lang.tr("Permisos ÚNICOS (herencia ya rota)", "UNIQUE permissions (inheritance already broken)")
                    .to_string()
            } else {
                match lang {
                    Lang::Es => format!("Hereda actualmente de: {}", inheritance.inherited_from_label),
                    Lang::En => format!("Currently inherits from: {}", inheritance.inherited_from_label),
                }
            };

            let mut lines = vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled(
                        lang.tr("  Carpeta destino: ", "  Target folder: "),
                        Style::default().fg(Color::Gray),
                    ),
                    Span::styled(
                        format!("{}/{}", library_title, relative_folder_path),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(vec![
                    Span::styled(
                        lang.tr("  Estado actual:   ", "  Current state: "),
                        Style::default().fg(Color::Gray),
                    ),
                    Span::styled(
                        state_badge,
                        Style::default().fg(if *target_has_unique_roles {
                            Color::Yellow
                        } else {
                            Color::LightGreen
                        }),
                    ),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    match lang {
                        Lang::Es => format!(
                            "  1. Grupo SharePoint (auto-crear si falta: {}): {}█",
                            create_group_if_missing, group_name
                        ),
                        Lang::En => format!(
                            "  1. SharePoint Group (auto-create if missing: {}): {}█",
                            create_group_if_missing, group_name
                        ),
                    },
                    field_style(0),
                )),
                Line::from(Span::styled(
                    match lang {
                        Lang::Es => format!("  2. Nivel de Permiso (←/→ para cambiar): < {} >", role_name),
                        Lang::En => format!("  2. Permission Level (←/→ to change): < {} >", role_name),
                    },
                    field_style(1),
                )),
                Line::from(Span::styled(
                    match lang {
                        Lang::Es => format!(
                            "  3. CopyParentRoles (copiar permisos heredados al romper): [{}]",
                            if *copy_parent_roles { yes } else { no }
                        ),
                        Lang::En => format!(
                            "  3. CopyParentRoles (copy inherited roles on break):       [{}]",
                            if *copy_parent_roles { yes } else { no }
                        ),
                    },
                    field_style(2),
                )),
                Line::from(Span::styled(
                    match lang {
                        Lang::Es => format!(
                            "  4. Reemplazar rol previo del mismo grupo (evita suma):     [{}]",
                            if *replace_inherited_roles { yes } else { no }
                        ),
                        Lang::En => format!(
                            "  4. Replace existing role for same group (avoid union):     [{}]",
                            if *replace_inherited_roles { yes } else { no }
                        ),
                    },
                    field_style(3),
                )),
                Line::from(Span::styled(
                    match lang {
                        Lang::Es => format!(
                            "  5. ClearSubscopes (resetear permisos únicos en subcarpetas):[{}]",
                            if *clear_subscopes { yes } else { no }
                        ),
                        Lang::En => format!(
                            "  5. ClearSubscopes (wipe unique roles on child folders):    [{}]",
                            if *clear_subscopes { yes } else { no }
                        ),
                    },
                    field_style(4),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    lang.tr(
                        "  ── Impacto en herencias ──",
                        "  ── Inheritance Impact ──",
                    ),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                )),
            ];

            if impact.disconnects_from_parent {
                lines.push(Line::from(Span::styled(
                    match lang {
                        Lang::Es => format!(
                            "  • Romperá la herencia desde '{}': cambios futuros arriba NO se propagarán aquí.",
                            inheritance.inherited_from_label
                        ),
                        Lang::En => format!(
                            "  • Will break inheritance from '{}': future parent changes will NOT propagate here.",
                            inheritance.inherited_from_label
                        ),
                    },
                    Style::default().fg(Color::Cyan),
                )));
            }
            if !impact.conflicting_roles.is_empty() {
                let old_str = impact.conflicting_roles.join(", ");
                if *replace_inherited_roles {
                    lines.push(Line::from(Span::styled(
                        match lang {
                            Lang::Es => format!(
                                "  ✓ '{}' ya tenía [{}]: se revocará antes de asignar [{}].",
                                group_name.trim(), old_str, role_name
                            ),
                            Lang::En => format!(
                                "  ✓ '{}' already had [{}]: will revoke it before granting [{}].",
                                group_name.trim(), old_str, role_name
                            ),
                        },
                        Style::default().fg(Color::LightGreen),
                    )));
                } else {
                    lines.push(Line::from(Span::styled(
                        match lang {
                            Lang::Es => format!(
                                "  ⚠ '{}' ya tiene [{}]: en SharePoint los roles se SUMAN si no los reemplazas.",
                                group_name.trim(), old_str
                            ),
                            Lang::En => format!(
                                "  ⚠ '{}' already has [{}]: SharePoint UNIONS roles unless replaced.",
                                group_name.trim(), old_str
                            ),
                        },
                        Style::default().fg(Color::Yellow),
                    )));
                }
            }
            if impact.will_lose_parent_roles {
                lines.push(Line::from(Span::styled(
                    lang.tr(
                        "  ⚠ CopyParentRoles=No eliminará aquí los permisos heredados de otros grupos (ej. Owners/Members).",
                        "  ⚠ CopyParentRoles=No will drop inherited permissions for all other groups on this folder.",
                    ),
                    Style::default().fg(Color::LightRed),
                )));
            }
            if impact.triggers_limited_access_upward {
                lines.push(Line::from(Span::styled(
                    lang.tr(
                        "  ℹ Este grupo no existe en niveles superiores: SharePoint añadirá 'Acceso limitado' arriba.",
                        "  ℹ Group is not on parent scopes: SharePoint will grant 'Limited Access' on ancestors.",
                    ),
                    Style::default().fg(Color::Gray),
                )));
            }
            if impact.wipes_child_unique_scopes {
                lines.push(Line::from(Span::styled(
                    lang.tr(
                        "  ⚠ ClearSubscopes=Sí borrará los permisos únicos ya configurados en subcarpetas descendientes.",
                        "  ⚠ ClearSubscopes=Yes will wipe existing unique permissions on descendant subfolders.",
                    ),
                    Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD),
                )));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                lang.tr(
                    "  [Tab / ↑ / ↓] Campo | [Espacio] Alternar | [←/→] Rol | [Enter] Encolar | [Esc] Cancelar",
                    "  [Tab / ↑ / ↓] Field | [Space] Toggle | [←/→] Role | [Enter] Queue | [Esc] Cancel",
                ),
                Style::default().fg(Color::Gray),
            )));

            let block = Block::default()
                .title(lang.tr(
                    " Asignar Permiso Único a Carpeta ",
                    " Grant Unique Folder Permission ",
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Yellow));
            f.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: false }), area);
        }
        Modal::ManageFolderPermissions {
            library_title,
            relative_folder_path,
            target_has_unique_roles,
            inheritance,
            entries,
            selected_idx,
        } => {
            let area = centered_rect(76, 66, f.area());
            f.render_widget(Clear, area);
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(5),
                    Constraint::Min(6),
                    Constraint::Length(3),
                ])
                .split(area);

            let note_line = if *target_has_unique_roles {
                Line::from(Span::styled(
                    lang.tr(
                        "  Esta carpeta tiene permisos únicos. Selecciona una asignación para revocarla:",
                        "  This folder has unique permissions. Select an assignment to revoke:",
                    ),
                    Style::default().fg(Color::Yellow),
                ))
            } else {
                Line::from(Span::styled(
                    match lang {
                        Lang::Es => format!(
                            "  ⚠ Hereda de '{}': revocar aquí romperá la herencia (CopyParentRoles=Sí) solo en esta carpeta.",
                            inheritance.inherited_from_label
                        ),
                        Lang::En => format!(
                            "  ⚠ Inherits from '{}': revoking here will break inheritance (CopyParentRoles=Yes) on this folder only.",
                            inheritance.inherited_from_label
                        ),
                    },
                    Style::default().fg(Color::Cyan),
                ))
            };

            let header = Paragraph::new(vec![
                Line::from(Span::styled(
                    format!("  {}/{}", library_title, relative_folder_path),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                )),
                note_line,
            ])
            .block(
                Block::default()
                    .title(lang.tr(" Revocar Permiso en Carpeta ", " Revoke Folder Permission "))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Yellow)),
            );
            f.render_widget(header, chunks[0]);

            let items: Vec<ListItem<'_>> = entries
                .iter()
                .enumerate()
                .map(|(idx, (principal, ptype, role))| {
                    let is_sel = idx == *selected_idx;
                    let style = if is_sel {
                        Style::default()
                            .bg(Color::Rgb(30, 58, 95))
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    ListItem::new(Line::from(Span::styled(
                        format!("  {} ({})  →  [{}]", principal, ptype, role),
                        style,
                    )))
                })
                .collect();
            let list_block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Cyan));
            render_scrollable_list(
                f,
                List::new(items).block(list_block),
                chunks[1],
                &app.scroll.modal_folder_perms,
                entries.len(),
                *selected_idx,
                true,
            );

            let footer = Paragraph::new(lang.tr(
                " [↑/↓] Mover | [Enter / d] Encolar revocación en Staging | [Esc] Cancelar",
                " [↑/↓] Move | [Enter / d] Queue revocation in Staging | [Esc] Cancel",
            ))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Gray)),
            );
            f.render_widget(footer, chunks[2]);
        }
        Modal::ConfirmResetFolderInheritance {
            library_title,
            relative_folder_path,
            inheritance,
        } => {
            let area = centered_rect(76, 46, f.area());
            f.render_widget(Clear, area);
            let inherited_roles_str = if inheritance.inherited_roles.is_empty() {
                lang.tr("(los roles actuales del padre)", "(current parent roles)").to_string()
            } else {
                inheritance
                    .inherited_roles
                    .iter()
                    .map(|r| format!("{}: [{}]", r.principal_title, r.roles.join(",")))
                    .collect::<Vec<_>>()
                    .join(" | ")
            };
            let lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    match lang {
                        Lang::Es => format!(
                            "  Carpeta: {}/{}",
                            library_title, relative_folder_path
                        ),
                        Lang::En => format!(
                            "  Folder:  {}/{}",
                            library_title, relative_folder_path
                        ),
                    },
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    match lang {
                        Lang::Es => format!(
                            "  Volverá a heredar permisos de: {}",
                            inheritance.inherited_from_label
                        ),
                        Lang::En => format!(
                            "  Will resume inheriting permissions from: {}",
                            inheritance.inherited_from_label
                        ),
                    },
                    Style::default().fg(Color::Yellow),
                )),
                Line::from(Span::styled(
                    format!("  ↳ {}", inherited_roles_str),
                    Style::default().fg(Color::Gray),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    lang.tr(
                        "  Los permisos únicos actuales de esta carpeta se perderán (reversible vía SQLite Rollback).",
                        "  Current unique permissions on this folder will be cleared (reversible via SQLite Rollback).",
                    ),
                    Style::default().fg(Color::White),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    match lang {
                        Lang::Es => "  ¿Encolar restauración de herencia?  [S / Enter] Sí   [N / Esc] No",
                        Lang::En => "  Queue inheritance reset?           [Y / Enter] Yes  [N / Esc] No",
                    },
                    Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD),
                )),
            ];
            let block = Block::default()
                .title(lang.tr(
                    " Restaurar Herencia de Carpeta ",
                    " Reset Folder Inheritance ",
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Yellow));
            f.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: false }), area);
        }
        Modal::ConfirmApplyStaging { note } => {
            let destructive_cnt = app.staging.destructive_ops_count();
            let is_destructive = destructive_cnt > 0;
            let area = centered_rect(76, if is_destructive { 66 } else { 60 }, f.area());
            f.render_widget(Clear, area);
            let mut lines = vec![Line::from("")];
            if is_destructive {
                lines.push(Line::from(Span::styled(
                    match lang {
                        Lang::Es => format!(
                            "  ⚠⚠⚠ ALERTA: {} operación(es) DESTRUCTIVA(S) borrarán datos/grupos en SharePoint (NO reversible) ⚠⚠⚠",
                            destructive_cnt
                        ),
                        Lang::En => format!(
                            "  ⚠⚠⚠ ALERT: {} DESTRUCTIVE operation(s) will delete data/groups in SharePoint (NOT reversible) ⚠⚠⚠",
                            destructive_cnt
                        ),
                    },
                    Style::default()
                        .fg(Color::White)
                        .bg(Color::Red)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
            }
            lines.push(Line::from(Span::styled(
                match lang {
                    Lang::Es => format!(
                        "  Se van a ejecutar {} operación(es) en SharePoint Online y registrar la transacción en SQLite:",
                        app.staging.len()
                    ),
                    Lang::En => format!(
                        "  About to execute {} operation(s) in SharePoint Online and record transaction in SQLite:",
                        app.staging.len()
                    ),
                },
                Style::default()
                    .fg(if is_destructive {
                        Color::LightRed
                    } else {
                        Color::LightGreen
                    })
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(match lang {
                Lang::Es => format!("  Resumen: {}", app.staging.build_summary_for(lang)),
                Lang::En => format!("  Summary: {}", app.staging.build_summary_for(lang)),
            }));
            lines.push(Line::from(""));

            for (i, op) in app.staging.ops.iter().take(8).enumerate() {
                let op_style = if op.is_destructive() {
                    Style::default()
                        .fg(Color::LightRed)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                lines.push(Line::from(Span::styled(
                    format!("   {:>2}. {}", i + 1, op.label_for(lang)),
                    op_style,
                )));
            }
            if app.staging.len() > 8 {
                lines.push(Line::from(match lang {
                    Lang::Es => format!("   ... y {} operación(es) más", app.staging.len() - 8),
                    Lang::En => format!("   ... and {} more operation(s)", app.staging.len() - 8),
                }));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled(
                    lang.tr("  Nota / Motivo (opcional): ", "  Note / Reason (optional): "),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(format!("{}█", note), Style::default().fg(Color::White)),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                lang.tr(
                    "  [Enter] APLICAR CAMBIOS AHORA   [Esc] Cancelar",
                    "  [Enter] APPLY CHANGES NOW   [Esc] Cancel",
                ),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )));

            let block = Block::default()
                .title(if is_destructive {
                    lang.tr(
                        " ⚠ CONFIRMAR APLICACIÓN DESTRUCTIVA (BORRARÁ DATOS EN DESTINO) ",
                        " ⚠ CONFIRM DESTRUCTIVE APPLY (WILL DELETE TARGET DATA) ",
                    )
                } else {
                    lang.tr(
                        " Confirmar Aplicación de Cambios (Transacción SQLite) ",
                        " Confirm Apply Changes (SQLite Transaction) ",
                    )
                })
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(if is_destructive {
                    Color::LightRed
                } else {
                    Color::LightGreen
                }));
            f.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: false }), area);
        }
        Modal::ConfirmRevertTransaction {
            tx_id,
            only_op_id,
            summary,
        } => {
            let area = centered_rect(65, 35, f.area());
            f.render_widget(Clear, area);
            let target_desc = match (only_op_id, lang) {
                (Some(op_id), Lang::Es) => format!("la operación #{} de la transacción #{}", op_id, tx_id),
                (Some(op_id), Lang::En) => format!("operation #{} of transaction #{}", op_id, tx_id),
                (None, Lang::Es) => format!("TODA la transacción #{} (en orden inverso LIFO)", tx_id),
                (None, Lang::En) => format!("ENTIRE transaction #{} (in reverse LIFO order)", tx_id),
            };
            let lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    match lang {
                        Lang::Es => format!("  ¿Seguro que deseas REVERTIR (Rollback) {}?", target_desc),
                        Lang::En => format!("  Are you sure you want to REVERT (Rollback) {}?", target_desc),
                    },
                    Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(match lang {
                    Lang::Es => format!("  Detalle: {}", summary),
                    Lang::En => format!("  Detail: {}", summary),
                }),
                Line::from(""),
                Line::from(Span::styled(
                    lang.tr(
                        "  [Enter / y / s] Sí, Revertir en SharePoint   [Esc / n] Cancelar",
                        "  [Enter / y] Yes, Revert in SharePoint   [Esc / n] Cancel",
                    ),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                )),
            ];
            let block = Block::default()
                .title(lang.tr(
                    " Confirmar Reversión (Rollback) ",
                    " Confirm Revert (Rollback) ",
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::LightRed));
            f.render_widget(Paragraph::new(lines).block(block), area);
        }
        Modal::SyncControlMenu {
            scope,
            selected_idx,
        } => {
            let area = centered_rect(78, 58, f.area());
            f.render_widget(Clear, area);

            let site_url = app
                .current_site
                .as_ref()
                .map(|s| s.url.as_str())
                .unwrap_or("");

            let (title, scope_desc, sync_state, od_state, combined_state) = match scope {
                SyncMenuScope::SelectedLibrary => {
                    let lib = app.selected_library();
                    let lib_name = lib.map(|l| l.title.as_str()).unwrap_or("-");
                    let sync_eff = lib
                        .map(|l| app.effective_library_sync_excluded(l))
                        .unwrap_or(false);
                    let od_eff = lib
                        .map(|l| app.effective_library_shortcuts_hidden(l))
                        .unwrap_or(false);
                    let sync_staged = lib
                        .and_then(|l| app.staging.library_sync_staging_state(site_url, &l.id))
                        .is_some();
                    let od_staged = lib
                        .and_then(|l| app.staging.library_shortcut_staging_state(site_url, &l.id))
                        .is_some();
                    let sync_star = if sync_staged { "*" } else { "" };
                    let od_star = if od_staged { "*" } else { "" };

                    let sync_txt = format!(
                        "{}{} -> {}",
                        if sync_eff {
                            lang.tr("Bloqueada", "Blocked")
                        } else {
                            lang.tr("Activa", "Active")
                        },
                        sync_star,
                        if sync_eff {
                            lang.tr("Permitir", "Allow")
                        } else {
                            lang.tr("Bloquear", "Block")
                        }
                    );
                    let od_txt = format!(
                        "{}{} -> {}",
                        if od_eff {
                            lang.tr("Oculto", "Hidden")
                        } else {
                            lang.tr("Visible", "Visible")
                        },
                        od_star,
                        if od_eff {
                            lang.tr("Mostrar", "Show")
                        } else {
                            lang.tr("Ocultar", "Hide")
                        }
                    );
                    let both_blocked = sync_eff && od_eff;
                    let comb_txt = if both_blocked {
                        lang.tr("Permitir ambos", "Allow both").to_string()
                    } else {
                        lang.tr("Bloquear ambos", "Block both").to_string()
                    };
                    (
                        match lang {
                            Lang::Es => format!(" Control Sync / Accesos OneDrive — Biblioteca '{}' ", lib_name),
                            Lang::En => format!(" Sync / OneDrive Shortcuts Control — Library '{}' ", lib_name),
                        },
                        match lang {
                            Lang::Es => format!("Ámbito: Biblioteca '{}'", lib_name),
                            Lang::En => format!("Scope: Library '{}'", lib_name),
                        },
                        sync_txt,
                        od_txt,
                        comb_txt,
                    )
                }
                SyncMenuScope::SiteAndLibraries => {
                    let site_name = app
                        .current_site
                        .as_ref()
                        .map(|s| s.title.as_str())
                        .unwrap_or("-");
                    let sync_eff = app.effective_all_libraries_sync_excluded();
                    let od_eff = app.effective_all_libraries_shortcuts_hidden();
                    let sync_staged = app.staging.site_sync_staging_state(site_url).is_some()
                        || app
                            .libraries
                            .iter()
                            .any(|l| app.staging.library_sync_staging_state(site_url, &l.id).is_some());
                    let od_staged = app.libraries.iter().any(|l| {
                        app.staging
                            .library_shortcut_staging_state(site_url, &l.id)
                            .is_some()
                    });
                    let sync_star = if sync_staged { "*" } else { "" };
                    let od_star = if od_staged { "*" } else { "" };

                    let sync_txt = format!(
                        "{}{} -> {}",
                        if sync_eff {
                            lang.tr("Bloqueada", "Blocked")
                        } else {
                            lang.tr("Activa", "Active")
                        },
                        sync_star,
                        if sync_eff {
                            lang.tr("Permitir", "Allow")
                        } else {
                            lang.tr("Bloquear", "Block")
                        }
                    );
                    let od_txt = format!(
                        "{}{} -> {}",
                        if od_eff {
                            lang.tr("Ocultos", "Hidden")
                        } else {
                            lang.tr("Visibles", "Visible")
                        },
                        od_star,
                        if od_eff {
                            lang.tr("Mostrar", "Show")
                        } else {
                            lang.tr("Ocultar", "Hide")
                        }
                    );
                    let both_blocked = sync_eff
                        && (app.libraries.is_empty() || od_eff);
                    let comb_txt = if both_blocked {
                        lang.tr("Permitir ambos", "Allow both").to_string()
                    } else {
                        lang.tr("Bloquear ambos", "Block both").to_string()
                    };
                    (
                        match lang {
                            Lang::Es => format!(" Control Sync / Accesos OneDrive — Sitio '{}' ", site_name),
                            Lang::En => format!(" Sync / OneDrive Shortcuts Control — Site '{}' ", site_name),
                        },
                        match lang {
                            Lang::Es => format!(
                                "Ámbito: Sitio '{}' y sus {} biblioteca(s)",
                                site_name,
                                app.libraries.len()
                            ),
                            Lang::En => format!(
                                "Scope: Site '{}' and its {} library/libraries",
                                site_name,
                                app.libraries.len()
                            ),
                        },
                        sync_txt,
                        od_txt,
                        comb_txt,
                    )
                }
            };

            let options = match lang {
                Lang::Es => [
                    format!(
                        "  [1] Sincronización SharePoint (ExcludeFromOfflineClient)   [{}]",
                        sync_state
                    ),
                    format!(
                        "  [2] Accesos directos a OneDrive (Botón barra de comandos)  [{}]",
                        od_state
                    ),
                    format!(
                        "  [3] Ambos (Sincronización SharePoint + Accesos OneDrive)   [{}]",
                        combined_state
                    ),
                ],
                Lang::En => [
                    format!(
                        "  [1] SharePoint Sync (ExcludeFromOfflineClient)             [{}]",
                        sync_state
                    ),
                    format!(
                        "  [2] OneDrive Shortcuts (Command bar button)                [{}]",
                        od_state
                    ),
                    format!(
                        "  [3] Both (SharePoint Sync + OneDrive Shortcuts)            [{}]",
                        combined_state
                    ),
                ],
            };

            let mut lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    format!("  {}", scope_desc),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                )),
                Line::from(lang.tr(
                    "  Selecciona qué subopción quieres alternar en Staging:",
                    "  Select which suboption to toggle in Staging:",
                )),
                Line::from(""),
            ];

            for (idx, text) in options.iter().enumerate() {
                let style = if idx == *selected_idx {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                lines.push(Line::from(Span::styled(text.clone(), style)));
            }

            lines.push(Line::from(""));

            lines.push(Line::from(Span::styled(
                lang.tr(
                    "  [↑/↓] Mover | [Enter / 1 / 2 / 3] Encolar en Staging | [Esc] Cancelar",
                    "  [↑/↓] Move | [Enter / 1 / 2 / 3] Queue in Staging | [Esc] Cancel",
                ),
                Style::default().fg(Color::Gray),
            )));

            let block = Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Cyan));
            f.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: false }), area);
        }
        Modal::ConfirmDestructiveClone {
            source_url,
            target_url,
            typed_confirm,
        } => {
            let area = centered_rect(78, 56, f.area());
            f.render_widget(Clear, area);
            let lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    lang.tr(
                        "  ⚠⚠⚠ ADVERTENCIA: CLONADO EN MODO DESTRUCTIVO ⚠⚠⚠",
                        "  ⚠⚠⚠ WARNING: DESTRUCTIVE CLONE MODE ⚠⚠⚠",
                    ),
                    Style::default()
                        .fg(Color::White)
                        .bg(Color::Red)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(match lang {
                    Lang::Es => format!("  Sitio ORIGEN:  {}", source_url),
                    Lang::En => format!("  SOURCE Site: {}", source_url),
                }),
                Line::from(Span::styled(
                    match lang {
                        Lang::Es => format!("  Sitio DESTINO: {}", target_url),
                        Lang::En => format!("  TARGET Site: {}", target_url),
                    },
                    Style::default()
                        .fg(Color::LightRed)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(lang.tr(
                    "  Al aplicar el plan en Staging se ELIMINARÁN en el sitio DESTINO:",
                    "  When applying the Staging plan, the following will be DELETED at TARGET:",
                )),
                Line::from(lang.tr(
                    "    • Bibliotecas no predeterminadas, carpetas y archivos actuales",
                    "    • Non-default libraries, existing folders and files",
                )),
                Line::from(lang.tr(
                    "    • Grupos personalizados no presentes en origen y miembros actuales",
                    "    • Custom groups missing in source and current group members",
                )),
                Line::from(Span::styled(
                    lang.tr(
                        "  Estas eliminaciones NO son reversibles mediante Rollback en SQLite.",
                        "  These deletions are NOT reversible via SQLite Rollback.",
                    ),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(vec![
                    Span::styled(
                        lang.tr(
                            "  Escribe BORRAR para generar el plan en Staging: ",
                            "  Type DELETE to generate the Staging plan: ",
                        ),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{}█", typed_confirm),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    lang.tr(
                        "  [Enter] Generar plan en Staging   [Esc] Cancelar",
                        "  [Enter] Generate Staging plan   [Esc] Cancel",
                    ),
                    Style::default().fg(Color::Gray),
                )),
            ];
            let block = Block::default()
                .title(lang.tr(
                    " ⚠ CONFIRMAR CLONADO DESTRUCTIVO ⚠ ",
                    " ⚠ CONFIRM DESTRUCTIVE CLONE ⚠ ",
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::LightRed));
            f.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: false }), area);
        }
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
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
    use crate::app::{App, Screen};
    use crate::auth::AuthManager;
    use crate::db::Database;
    use crate::models::{SiteRole, SpoSite};
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

        // 80x16 => header(3) + main(10) + footer(3); table viewport_rows = 10 - 4 = 6 < 20
        let backend = TestBackend::new(80, 16);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|f| draw_ui(f, &app)).unwrap();
        let right_col = right_column_symbols(terminal.backend().buffer());
        assert!(right_col.contains('▲'), "expected top scrollbar arrow in {right_col:?}");
        assert!(right_col.contains('▼'), "expected bottom scrollbar arrow in {right_col:?}");
        assert!(right_col.contains('█'), "expected scrollbar thumb in {right_col:?}");

        // Scroll selection down to index 18 -> must bring "Site 18" into view (rows 13..=18)
        app.selected_site_idx = 18;
        terminal.draw(|f| draw_ui(f, &app)).unwrap();
        assert_eq!(app.scroll.sites.get(), 13);
        let rendered = buffer_text(terminal.backend().buffer());
        assert!(rendered.contains("Site 13"));
        assert!(rendered.contains("Site 18"));
        assert!(!rendered.contains("Site 00"));
        assert!(!rendered.contains("Site 19"));

        // Resize taller to 80x35 => main(29), viewport_rows = 25 >= 20 -> scrollbar must disappear
        terminal.backend_mut().resize(80, 35);
        terminal.draw(|f| draw_ui(f, &app)).unwrap();
        assert_eq!(app.scroll.sites.get(), 0);
        let right_col_tall = right_column_symbols(terminal.backend().buffer());
        assert!(!right_col_tall.contains('▲'));
        assert!(!right_col_tall.contains('▼'));
        let rendered_tall = buffer_text(terminal.backend().buffer());
        assert!(rendered_tall.contains("Site 00"));
        assert!(rendered_tall.contains("Site 19"));

        // Resize back down to 80x16 -> scrollbar must reappear dynamically
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
        assert!(
            rendered.contains("[Sync: BLOQUEADA]"),
            "expected site blocked sync badge in:\n{rendered}"
        );
        assert!(
            rendered.contains("[NoSync]"),
            "expected library [NoSync] tag in:\n{rendered}"
        );
        assert!(
            rendered.contains("[NoOD]"),
            "expected library [NoOD] tag in:\n{rendered}"
        );
        assert!(
            rendered.contains("[Sync]"),
            "expected library [Sync] tag in:\n{rendered}"
        );
        assert!(
            rendered.contains("[OD]"),
            "expected library [OD] tag in:\n{rendered}"
        );

        // Now verify active site badge, staged library sync disable, and staged OneDrive shortcut hide
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
        assert!(
            rendered_staged.contains("[Sync: ACTIVA]"),
            "expected site active sync badge in:\n{rendered_staged}"
        );
        assert!(
            rendered_staged.contains("[-Sync*]"),
            "expected staged library [-Sync*] tag in:\n{rendered_staged}"
        );
        assert!(
            rendered_staged.contains("[-OD*]"),
            "expected staged library [-OD*] tag in:\n{rendered_staged}"
        );

        // Verify SyncControlMenu modal renders both suboptions
        app.open_sync_menu(SyncMenuScope::SelectedLibrary);
        terminal.draw(|f| draw_ui(f, &app)).unwrap();
        let rendered_modal = buffer_text(terminal.backend().buffer());
        assert!(
            rendered_modal.contains("[1] Sincronización SharePoint"),
            "expected SharePoint sync option in modal:\n{rendered_modal}"
        );
        assert!(
            rendered_modal.contains("[2] Accesos directos a OneDrive"),
            "expected OneDrive shortcuts option in modal:\n{rendered_modal}"
        );
        assert!(
            rendered_modal.contains("[3] Ambos"),
            "expected combined option in modal:\n{rendered_modal}"
        );
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
        assert!(
            rendered_tab.contains("MODO DESTRUCTIVO ACTIVADO"),
            "expected destructive mode warning banner in:\n{rendered_tab}"
        );

        app.modal = Modal::ConfirmDestructiveClone {
            source_url: app.clone_source_url.clone(),
            target_url: site_url.clone(),
            typed_confirm: "BOR".to_string(),
        };
        terminal.draw(|f| draw_ui(f, &app)).unwrap();
        let rendered_confirm = buffer_text(terminal.backend().buffer());
        assert!(
            rendered_confirm.contains("CONFIRMAR CLONADO DESTRUCTIVO"),
            "expected destructive clone modal in:\n{rendered_confirm}"
        );
        assert!(
            rendered_confirm.contains("Escribe BORRAR"),
            "expected BORRAR prompt in:\n{rendered_confirm}"
        );

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
        assert!(
            rendered_apply.contains("CONFIRMAR APLICACIÓN DESTRUCTIVA"),
            "expected destructive apply warning in:\n{rendered_apply}"
        );
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
        assert!(
            auth_txt.contains("Microsoft 365 Authentication"),
            "expected English auth title:\n{auth_txt}"
        );

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
        assert!(
            clone_txt.contains("DESTRUCTIVE MODE ENABLED"),
            "expected English destructive warning:\n{clone_txt}"
        );

        app.modal = Modal::ConfirmDestructiveClone {
            source_url: "https://contoso.sharepoint.com/sites/Source".to_string(),
            target_url: "https://contoso.sharepoint.com/sites/Target".to_string(),
            typed_confirm: String::new(),
        };
        terminal.draw(|f| draw_ui(f, &app)).unwrap();
        let modal_txt = buffer_text(terminal.backend().buffer());
        assert!(
            modal_txt.contains("Type DELETE to generate the Staging plan"),
            "expected English DELETE prompt:\n{modal_txt}"
        );
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
        app.selected_folder_idx = 1; // Row 0 is "..", Row 1 is "Deep"
        assert_eq!(app.folder_list_row_count(), 2);
        assert_eq!(app.selected_folder().unwrap().name, "Deep");

        let backend = TestBackend::new(140, 32);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|f| draw_ui(f, &app)).unwrap();
        let tree_txt = buffer_text(terminal.backend().buffer());
        assert!(tree_txt.contains("[Nivel 2]"), "expected depth badge:\n{tree_txt}");
        assert!(
            tree_txt.contains("Documentos / 01_Dir / Sub"),
            "expected breadcrumb:\n{tree_txt}"
        );
        assert!(
            tree_txt.contains("Hereda de: 01_Dir"),
            "expected ancestor inheritance source:\n{tree_txt}"
        );
        assert!(tree_txt.contains(".."), "expected parent '..' row:\n{tree_txt}");
        assert!(
            tree_txt.contains("Deep (3)"),
            "expected subfolder item with count:\n{tree_txt}"
        );

        // GrantFolderPermission modal shows impact warnings
        let inh = FolderInheritanceSummary::from_ancestors(&app.folder_ancestors, Lang::Es);
        app.modal = Modal::GrantFolderPermission {
            library_title: "Documentos".to_string(),
            relative_folder_path: "01_Dir/Sub/Deep".to_string(),
            target_has_unique_roles: false,
            target_current_roles: Vec::new(),
            inheritance: inh.clone(),
            group_name: "Members".to_string(),
            role_idx: 0, // Read (conflicts with inherited Edit)
            copy_parent_roles: true,
            replace_inherited_roles: true,
            clear_subscopes: true,
            create_group_if_missing: true,
            active_field: 0,
        };
        terminal.draw(|f| draw_ui(f, &app)).unwrap();
        let grant_txt = buffer_text(terminal.backend().buffer());
        assert!(
            grant_txt.contains("Impacto en herencias"),
            "expected inheritance impact section:\n{grant_txt}"
        );
        assert!(
            grant_txt.contains("se revocará antes de asignar [Read]"),
            "expected conflict replacement notice:\n{grant_txt}"
        );
        assert!(
            grant_txt.contains("ClearSubscopes=Sí borrará"),
            "expected clear_subscopes warning:\n{grant_txt}"
        );

        // ManageFolderPermissions modal in English
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
        assert!(
            revoke_txt.contains("Revoke Folder Permission"),
            "expected revoke modal title:\n{revoke_txt}"
        );
        assert!(
            revoke_txt.contains("Members (SharePointGroup)"),
            "expected entry in revoke modal:\n{revoke_txt}"
        );

        // ConfirmResetFolderInheritance modal
        app.modal = Modal::ConfirmResetFolderInheritance {
            library_title: "Documentos".to_string(),
            relative_folder_path: "01_Dir/Sub".to_string(),
            inheritance: inh,
        };
        terminal.draw(|f| draw_ui(f, &app)).unwrap();
        let reset_txt = buffer_text(terminal.backend().buffer());
        assert!(
            reset_txt.contains("Reset Folder Inheritance"),
            "expected reset inheritance modal:\n{reset_txt}"
        );
        assert!(
            reset_txt.contains("Will resume inheriting permissions from: 01_Dir"),
            "expected parent inheritance source in reset modal:\n{reset_txt}"
        );
    }
}
