use crate::app::{
    App, Modal, PaneFocus, Screen, WorkspaceTab, STANDARD_ROLES,
};
use crate::models::{GroupKind, SiteRole};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Row, Table, Tabs, Wrap},
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

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Bienvenido a SPOIberia — Gestión de Autorizaciones, Carpetas y Clonado en SharePoint Online",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("Selecciona un método de inicio de sesión nativo contra Microsoft Entra ID:"),
        Line::from(""),
        Line::from(vec![
            Span::styled("  [Enter] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("Iniciar sesión interactivo en el navegador (OAuth2 + PKCE)"),
        ]),
        Line::from(vec![
            Span::styled("  [  d  ] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw("Iniciar sesión mediante Código de Dispositivo (Device Code Flow)"),
        ]),
        Line::from(vec![
            Span::styled("  [  q  ] ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
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

    f.render_widget(table, area);
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
    f.render_widget(List::new(group_items).block(groups_block), panes[0]);

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

    f.render_widget(members_table, panes[1]);
}

fn draw_tab_libraries_and_folders(f: &mut Frame<'_>, app: &App, area: Rect) {
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(34), Constraint::Percentage(66)])
        .split(area);

    let left_focused = app.focus == PaneFocus::Left;
    let right_focused = app.focus == PaneFocus::Right;

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
            let def_tag = if l.is_default_documents {
                "[Default] "
            } else {
                ""
            };
            ListItem::new(Line::from(vec![
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
        .title(" Bibliotecas [b: Nueva Biblioteca] ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if left_focused {
            Color::Cyan
        } else {
            Color::DarkGray
        }));
    f.render_widget(List::new(lib_items).block(lib_block), panes[0]);

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

    f.render_widget(folders_table, panes[1]);
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
                let fg = if label.starts_with("[+]") || label.starts_with("[+Rol]") {
                    Color::LightGreen
                } else if label.starts_with("[-]") {
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
    f.render_widget(List::new(staging_items).block(staging_block), panes[0]);

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
    f.render_widget(tx_table, right_split[0]);

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
    f.render_widget(List::new(op_items).block(ops_block), right_split[1]);
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
        Screen::Auth => "Enter: Login Navegador | d: Login Device Code | q: Salir",
        Screen::SiteSelector => {
            "↑/↓: Mover | Enter: Abrir Sitio | /: Filtrar | s: Buscar en Tenant | u: URL Manual | c: Fijar Origen Clon | r: Recargar | ?: Ayuda | q: Salir"
        }
        Screen::SiteWorkspace => match app.tab {
            WorkspaceTab::GroupsAndUsers => {
                "Tab/←/→: Cambiar Panel | a: Asignar Usuario | d: Remover Usuario | g: Crear Grupo | A: APLICAR Staging | H: Historial/Rollback | Esc: Sitios"
            }
            WorkspaceTab::LibrariesAndFolders => {
                "Tab/←/→: Panel | n: Crear Carpeta | b: Crear Biblioteca | p: Asignar Permiso | i: Importar CSV/JSON | e: Exportar | A: APLICAR | Esc: Sitios"
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
            let area = centered_rect(72, 75, f.area());
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
                Line::from("• Módulo 2 — Bibliotecas, Carpetas y Matriz CSV/JSON (Pestaña 2):"),
                Line::from("  n / b       Crear nueva carpeta (n) o nueva biblioteca (b)"),
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
            f.render_widget(List::new(items).block(list_block), chunks[1]);

            let help_block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded);
            f.render_widget(
                Paragraph::new(
                    " [Ctrl+S / Tab] Buscar ahora | [↑/↓] Mover | [Espacio] Marcar varios | [Enter] Encolar en Staging | [Esc] Cancelar",
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
