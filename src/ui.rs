use crate::app::{
    App, Modal, PaneFocus, Screen, SyncMenuScope, WorkspaceTab, STANDARD_ROLES,
};
use crate::models::{GroupKind, SiteRole};
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
    let pending_cnt = app.staging.len();
    let staging_span = if pending_cnt > 0 {
        Span::styled(
            format!(" ● [Staging: {} cambio(s) pendiente(s) — Pulsa 'A' para Aplicar] ", pending_cnt),
            Style::default()
                .fg(Color::Black)
                .bg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            " [Staging: 0 cambios] ",
            Style::default().fg(Color::DarkGray),
        )
    };

    let user_label = if app.user_upn.is_empty() {
        "No autenticado".to_string()
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
            " [Sync: BLOQUEAR*] ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
        ),
        Some(false) => Span::styled(
            " [Sync: ACTIVAR*] ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        ),
        None => match app.site_exclude_from_offline_client {
            Some(true) => Span::styled(
                " [Sync: BLOQUEADA] ",
                Style::default()
                    .fg(Color::LightRed)
                    .add_modifier(Modifier::BOLD),
            ),
            Some(false) => Span::styled(
                " [Sync: ACTIVA] ",
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
            format!(" Usuario: {} ", user_label),
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
    let block = Block::default()
        .title(" Autenticación Microsoft 365 (Entra ID OAuth2) ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan));

    let email_display = if app.login_email_input.is_empty() {
        Span::styled(
            "usuario@dominio.com█",
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
            "Bienvenido a SPOIberia — Gestión de Autorizaciones, Carpetas y Clonado en SharePoint Online",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("Introduce tu correo corporativo M365 (para detectar automáticamente el tenant):"),
        Line::from(vec![
            Span::styled("  Correo / Dominio / Tenant: [ ", Style::default().fg(Color::Cyan)),
            email_display,
            Span::styled(" ]", Style::default().fg(Color::Cyan)),
        ]),
        Line::from(""),
        Line::from("Selecciona un método de inicio de sesión nativo contra Microsoft Entra ID:"),
        Line::from(""),
        Line::from(vec![
            Span::styled("  [    Enter    ] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("Iniciar sesión interactivo en el navegador (OAuth2 + PKCE)"),
        ]),
        Line::from(vec![
            Span::styled("  [ F2 / Ctrl+D ] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw("Iniciar sesión mediante Código de Dispositivo (Device Code Flow)"),
        ]),
        Line::from(vec![
            Span::styled("  [     Esc     ] ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw("Salir"),
        ]),
    ];

    if let Some(ref prompt) = app.device_code_prompt {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "─── CÓDIGO DE DISPOSITIVO ACTIVO ───",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(format!("1. Abre en tu navegador: {}", prompt.verification_uri)));
        lines.push(Line::from(vec![
            Span::raw("2. Introduce el código:  "),
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
    let filtered = app.filtered_sites();
    let filter_info = if app.site_filter.is_empty() {
        String::new()
    } else {
        format!(" [Filtro: '{}']", app.site_filter)
    };
    let clone_src_info = if app.clone_source_url.is_empty() {
        String::new()
    } else {
        format!(" [Origen Clonado: {}]", app.clone_source_url)
    };

    let title = format!(
        " Sitios SharePoint donde eres Administrador o Propietario ({} sitios){}{} ",
        filtered.len(),
        filter_info,
        clone_src_info
    );

    let header = Row::new(vec!["Rol", "Título del Sitio", "URL de SharePoint Online"])
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
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(8)])
        .split(area);

    let tab_titles = vec![
        format!(" [1] Grupos y Usuarios ({}) ", app.groups.len()),
        format!(" [2] Bibliotecas y Carpetas ({}) ", app.libraries.len()),
        " [3] Clonar Sitio SharePoint ".to_string(),
        format!(
            " [4] Cola Staging ({}) e Historial SQLite ({}) ",
            app.staging.len(),
            app.transactions.len()
        ),
    ];

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
                .title(" Módulos del Sitio (Teclas 1, 2, 3, 4 | Esc: Volver a Sitios) "),
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
        .title(format!(
            " Grupos del Sitio ({}) [g: Nuevo Grupo] ",
            app.groups.len()
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
                "Pendiente Aplicar ('A')".to_string(),
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
                "[- BAJA]".to_string(),
                Style::default()
                    .fg(Color::LightRed)
                    .add_modifier(Modifier::CROSSED_OUT),
            ),
            Some(true) => (
                "[+ ALTA]".to_string(),
                Style::default().fg(Color::LightGreen),
            ),
            None => (
                if u.is_site_admin {
                    "[Admin]".to_string()
                } else {
                    "Activo".to_string()
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
        Row::new(vec!["Estado", "Nombre", "Email / UPN", "SP ID"])
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .bottom_margin(1),
    )
    .block(
        Block::default()
            .title(format!(
                " Miembros de '{}' ({}) [a: Asignar | d: Remover | A: Aplicar] ",
                current_grp_title,
                app.group_members.len()
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
                    format!("{} ({} ítems)", l.title, l.item_count),
                    Style::default()
                        .fg(Color::White)
                        .bg(bg)
                        .add_modifier(if is_sel { Modifier::BOLD } else { Modifier::empty() }),
                ),
            ]))
        })
        .collect();

    let lib_block = Block::default()
        .title(" Bibliotecas [b: Nueva | o: Sync/OneDrive] ")
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

    let rows: Vec<Row<'_>> = app
        .folders
        .iter()
        .enumerate()
        .map(|(idx, folder)| {
            let is_sel = idx == app.selected_folder_idx;
            let perms_desc = if folder.role_assignments.is_empty() {
                if folder.has_unique_role_assignments {
                    "Permisos únicos".to_string()
                } else {
                    "Hereda de biblioteca".to_string()
                }
            } else {
                folder
                    .role_assignments
                    .iter()
                    .map(|r| format!("{}: [{}]", r.principal_title, r.roles.join(",")))
                    .collect::<Vec<_>>()
                    .join(" | ")
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

            Row::new(vec![
                folder.relative_path.clone(),
                if folder.has_unique_role_assignments {
                    "Únicos (Rota)".to_string()
                } else {
                    "Heredados".to_string()
                },
                perms_desc,
            ])
            .style(style)
        })
        .collect();

    let folders_table = Table::new(
        rows,
        [
            Constraint::Percentage(32),
            Constraint::Length(15),
            Constraint::Percentage(53),
        ],
    )
    .header(
        Row::new(vec![
            "Carpeta (Ruta Relativa)",
            "Herencia",
            "Grupos y Permisos Asignados",
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
            .title(format!(
                " Carpetas en '{}' [n: Crear Carpeta | p: Asignar Permiso | i: Importar CSV/JSON | e: Exportar] ",
                current_lib
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
        panes[1],
        &app.scroll.folders,
        app.folders.len(),
        app.selected_folder_idx,
        right_focused,
    );
}

fn draw_tab_clone_site(f: &mut Frame<'_>, app: &App, area: Rect) {
    let target_url = app
        .current_site
        .as_ref()
        .map(|s| s.url.as_str())
        .unwrap_or("-");

    let src_display = if app.clone_source_url.is_empty() {
        "(No seleccionado — pulsa 's' para escribir URL o 'c' en el Selector de Sitios)".to_string()
    } else {
        app.clone_source_url.clone()
    };

    let options = [
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
    ];

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Sitio ORIGEN  (Source): ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(src_display, Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::styled("  Sitio DESTINO (Target): ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled(target_url, Style::default().fg(Color::White)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Opciones de Clonado (Usa ↑/↓ y Espacio para marcar/desmarcar):",
            Style::default().fg(Color::Cyan),
        )),
        Line::from(""),
    ];

    for (idx, opt_text) in options.iter().enumerate() {
        let is_sel = idx == app.clone_selected_option;
        let style = if is_sel {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(opt_text.clone(), style),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(
            "  [s] ",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ),
        Span::raw("Indicar URL del Sitio Origen   "),
        Span::styled(
            "  [Enter] ",
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        ),
        Span::raw("Generar Plan de Clonado en Cola Staging (para revisar y Aplicar con 'A')"),
    ]));

    let block = Block::default()
        .title(" Fase 3: Clonado Completo de un Sitio SharePoint en otro ")
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
            "  (Cola de Staging vacía — añade/quita usuarios o importa un CSV/JSON)",
            Style::default().fg(Color::DarkGray),
        )))]
    } else {
        app.staging
            .ops
            .iter()
            .enumerate()
            .map(|(idx, op)| {
                let is_sel = idx == app.selected_staging_idx && left_focused;
                let label = op.label();
                let fg = if label.starts_with("[+]")
                    || label.starts_with("[+Rol]")
                    || label.starts_with("[+Sync]")
                    || label.starts_with("[+AtajoOD]")
                {
                    Color::LightGreen
                } else if label.starts_with("[-]")
                    || label.starts_with("[-Sync]")
                    || label.starts_with("[-AtajoOD]")
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
        .title(format!(
            " Cola Staging ({}) [A: Aplicar Cambios | d: Quitar ítem | X: Vaciar] ",
            app.staging.len()
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
        Row::new(vec!["ID", "Fecha (UTC)", "Estado", "Resumen de Acciones"])
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .bottom_margin(1),
    )
    .block(
        Block::default()
            .title(format!(
                " Historial de Transacciones SQLite ({}) [R: Revertir Transacción] ",
                app.transactions.len()
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
        .title(" Operaciones Atómicas de la Transacción Seleccionada [r: Revertir Operación] ")
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
                " ⏳ PROCESANDO: ",
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
    match app.screen {
        Screen::Auth => "Escribe tu correo M365 | Enter: Login Navegador | F2 / Ctrl+D: Device Code | Esc: Salir",
        Screen::SiteSelector => {
            "↑/↓: Mover | Enter: Abrir Sitio | /: Filtrar | s: Buscar en Tenant | u: URL Manual | c: Fijar Origen Clon | r: Recargar | ?: Ayuda | q: Salir"
        }
        Screen::SiteWorkspace => match app.tab {
            WorkspaceTab::GroupsAndUsers => {
                "Tab/←/→: Cambiar Panel | a: Asignar Usuario | d: Remover Usuario | g: Crear Grupo | O: Menú Sync/OneDrive Sitio | A: APLICAR Staging | H: Historial/Rollback | Esc: Sitios"
            }
            WorkspaceTab::LibrariesAndFolders => {
                "Tab/←/→: Panel | o: Menú Sync/OneDrive Bib | O: Menú Sitio+Bibs | n: Carpeta | b: Bib | p: Permiso | i/e: CSV/JSON | A: APLICAR"
            }
            WorkspaceTab::CloneSite => {
                "↑/↓: Mover Opción | Espacio: Activar/Desactivar | s: Cambiar URL Origen | Enter: Generar Plan en Staging | A: APLICAR | Esc: Sitios"
            }
            WorkspaceTab::StagingAndHistory => {
                "Tab/←/→: Staging vs Historial | A: APLICAR Staging | R: Revertir Transacción | r: Revertir Op | d: Quitar de Staging | X: Vaciar Staging"
            }
        },
    }
}

fn draw_modal(f: &mut Frame<'_>, app: &App) {
    match &app.modal {
        Modal::None => {}
        Modal::Help => {
            let area = centered_rect(76, 82, f.area());
            f.render_widget(Clear, area);
            let lines = vec![
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
                Line::from("  n / b       Crear nueva carpeta (n) o nueva biblioteca (b)"),
                Line::from("  o / O       Abrir menú Sync SharePoint / Accesos OneDrive en biblioteca (o) o sitio+bibs (O)"),
                Line::from("  p           Romper herencia y asignar rol (Read/Contribute/Edit/Full Control) a un grupo"),
                Line::from("  i / e       Importar matriz de carpetas/permisos CSV/JSON (i) o exportar (e)"),
                Line::from(""),
                Line::from("• Staging y Transacciones SQLite (Pestaña 4 / Global):"),
                Line::from("  A           APLICAR todos los cambios en Staging y registrar transacción en SQLite"),
                Line::from("  R / r       Revertir (Rollback) una transacción completa (R) o una operación (r)"),
                Line::from(""),
                Line::from(Span::styled("Pulsa Esc o ? para cerrar esta ayuda", Style::default().fg(Color::Yellow))),
            ];
            let block = Block::default()
                .title(" Ayuda de SPOIberia ")
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
                    " [Enter] Confirmar   [Esc] Cancelar",
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

            let status = if *searching { " (Buscando...)" } else { "" };
            let search_block = Block::default()
                .title(format!(
                    " Asignar Usuario(s) al Grupo '{}'{} ",
                    group_title, status
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::LightGreen));
            let search_line = Line::from(vec![
                Span::styled(" Buscar en directorio (o escribe email/UPN): ", Style::default().fg(Color::Cyan)),
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
                .title(format!(
                    " Resultados del Directorio ({}) — Seleccionados: {} ",
                    results.len(),
                    marked_identifiers.len()
                ))
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
                Paragraph::new(
                    " [↑/↓] Mover | [Tab] Marcar varios | [Ctrl+S] Buscar ahora | [Enter] Encolar en Staging | [Esc] Cancelar",
                )
                .block(help_block),
                chunks[2],
            );
        }
        Modal::GrantFolderPermission {
            library_title,
            relative_folder_path,
            group_name,
            role_idx,
            copy_parent_roles,
            clear_subscopes,
            create_group_if_missing,
            active_field,
        } => {
            let area = centered_rect(70, 55, f.area());
            f.render_widget(Clear, area);
            let role_name = STANDARD_ROLES.get(*role_idx).copied().unwrap_or("Read");

            let f0_style = if *active_field == 0 {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let f1_style = if *active_field == 1 {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let f2_style = if *active_field == 2 {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let f3_style = if *active_field == 3 {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let lines = vec![
                Line::from(""),
                Line::from(format!(
                    "  Carpeta destino: {}/{}",
                    library_title, relative_folder_path
                )),
                Line::from(""),
                Line::from(Span::styled(
                    format!("  1. Grupo SharePoint (se creará si no existe: {}): {}█", create_group_if_missing, group_name),
                    f0_style,
                )),
                Line::from(""),
                Line::from(Span::styled(
                    format!("  2. Nivel de Permiso (←/→ para cambiar): < {} >", role_name),
                    f1_style,
                )),
                Line::from(""),
                Line::from(Span::styled(
                    format!("  3. CopyParentRoles al romper herencia (Espacio): [{}]", if *copy_parent_roles { "Sí" } else { "No" }),
                    f2_style,
                )),
                Line::from(""),
                Line::from(Span::styled(
                    format!("  4. ClearSubscopes al romper herencia (Espacio):  [{}]", if *clear_subscopes { "Sí" } else { "No" }),
                    f3_style,
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "  [Tab / ↑ / ↓] Cambiar campo | [Enter] Encolar en Staging | [Esc] Cancelar",
                    Style::default().fg(Color::Gray),
                )),
            ];

            let block = Block::default()
                .title(" Asignar Permiso Único a Carpeta ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Yellow));
            f.render_widget(Paragraph::new(lines).block(block), area);
        }
        Modal::ConfirmApplyStaging { note } => {
            let area = centered_rect(72, 60, f.area());
            f.render_widget(Clear, area);
            let mut lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    format!(
                        "  Se van a ejecutar {} operación(es) en SharePoint Online y registrar la transacción en SQLite:",
                        app.staging.len()
                    ),
                    Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD),
                )),
                Line::from(format!("  Resumen: {}", app.staging.build_summary())),
                Line::from(""),
            ];

            for (i, op) in app.staging.ops.iter().take(8).enumerate() {
                lines.push(Line::from(format!("   {:>2}. {}", i + 1, op.label())));
            }
            if app.staging.len() > 8 {
                lines.push(Line::from(format!(
                    "   ... y {} operación(es) más",
                    app.staging.len() - 8
                )));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("  Nota / Motivo (opcional): ", Style::default().fg(Color::Cyan)),
                Span::styled(format!("{}█", note), Style::default().fg(Color::White)),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  [Enter] APLICAR CAMBIOS AHORA   [Esc] Cancelar",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )));

            let block = Block::default()
                .title(" Confirmar Aplicación de Cambios (Transacción SQLite) ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::LightGreen));
            f.render_widget(Paragraph::new(lines).block(block), area);
        }
        Modal::ConfirmRevertTransaction {
            tx_id,
            only_op_id,
            summary,
        } => {
            let area = centered_rect(65, 35, f.area());
            f.render_widget(Clear, area);
            let target_desc = match only_op_id {
                Some(op_id) => format!("la operación #{} de la transacción #{}", op_id, tx_id),
                None => format!("TODA la transacción #{} (en orden inverso LIFO)", tx_id),
            };
            let lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    format!("  ¿Seguro que deseas REVERTIR (Rollback) {}?", target_desc),
                    Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(format!("  Detalle: {}", summary)),
                Line::from(""),
                Line::from(Span::styled(
                    "  [Enter / y] Sí, Revertir en SharePoint   [Esc / n] Cancelar",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                )),
            ];
            let block = Block::default()
                .title(" Confirmar Reversión (Rollback) ")
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
                        if sync_eff { "Bloqueada" } else { "Activa" },
                        sync_star,
                        if sync_eff { "Permitir" } else { "Bloquear" }
                    );
                    let od_txt = format!(
                        "{}{} -> {}",
                        if od_eff { "Oculto" } else { "Visible" },
                        od_star,
                        if od_eff { "Mostrar" } else { "Ocultar" }
                    );
                    let both_blocked = sync_eff && od_eff;
                    let comb_txt = if both_blocked {
                        "Permitir ambos".to_string()
                    } else {
                        "Bloquear ambos".to_string()
                    };
                    (
                        format!(" Control Sync / Accesos OneDrive — Biblioteca '{}' ", lib_name),
                        format!("Ámbito: Biblioteca '{}'", lib_name),
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
                        if sync_eff { "Bloqueada" } else { "Activa" },
                        sync_star,
                        if sync_eff { "Permitir" } else { "Bloquear" }
                    );
                    let od_txt = format!(
                        "{}{} -> {}",
                        if od_eff { "Ocultos" } else { "Visibles" },
                        od_star,
                        if od_eff { "Mostrar" } else { "Ocultar" }
                    );
                    let both_blocked = sync_eff
                        && (app.libraries.is_empty() || od_eff);
                    let comb_txt = if both_blocked {
                        "Permitir ambos".to_string()
                    } else {
                        "Bloquear ambos".to_string()
                    };
                    (
                        format!(" Control Sync / Accesos OneDrive — Sitio '{}' ", site_name),
                        format!(
                            "Ámbito: Sitio '{}' y sus {} biblioteca(s)",
                            site_name,
                            app.libraries.len()
                        ),
                        sync_txt,
                        od_txt,
                        comb_txt,
                    )
                }
            };

            let options = [
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
            ];

            let mut lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    format!("  {}", scope_desc),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                )),
                Line::from("  Selecciona qué subopción quieres alternar en Staging:"),
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
                "  [↑/↓] Mover | [Enter / 1 / 2 / 3] Encolar en Staging | [Esc] Cancelar",
                Style::default().fg(Color::Gray),
            )));

            let block = Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Cyan));
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
        let app = App::new(auth, db, graph, spo);
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
}
