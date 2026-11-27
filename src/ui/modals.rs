use super::centered_rect;
use super::widgets::render_scrollable_list;
use crate::app::{App, Modal, SyncMenuScope, STANDARD_ROLES};
use crate::i18n::Lang;
use crate::models::{
    analyze_permission_change_impact, DirectoryUser, FolderInheritanceSummary,
    FolderPermissionEntry,
};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};
use std::collections::HashSet;

pub(super) fn draw_modal(f: &mut Frame<'_>, app: &App) {
    let lang = app.lang;
    match &app.modal {
        Modal::None => {}
        Modal::Help => draw_help_modal(f, lang),
        Modal::TextInput {
            title,
            prompt,
            value,
            ..
        } => draw_text_input_modal(f, lang, title, prompt, value),
        Modal::AddUserToGroup {
            group_title,
            query,
            results,
            selected_idx,
            marked_identifiers,
            searching,
            ..
        } => draw_add_user_modal(
            f,
            app,
            group_title,
            query,
            results,
            *selected_idx,
            marked_identifiers,
            *searching,
        ),
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
        } => draw_grant_permission_modal(
            f,
            lang,
            library_title,
            relative_folder_path,
            *target_has_unique_roles,
            target_current_roles,
            inheritance,
            group_name,
            *role_idx,
            *copy_parent_roles,
            *replace_inherited_roles,
            *clear_subscopes,
            *create_group_if_missing,
            *active_field,
        ),
        Modal::ManageFolderPermissions {
            library_title,
            relative_folder_path,
            target_has_unique_roles,
            inheritance,
            entries,
            selected_idx,
        } => draw_manage_folder_permissions_modal(
            f,
            app,
            library_title,
            relative_folder_path,
            *target_has_unique_roles,
            inheritance,
            entries,
            *selected_idx,
        ),
        Modal::ConfirmResetFolderInheritance {
            library_title,
            relative_folder_path,
            inheritance,
        } => draw_confirm_reset_inheritance_modal(
            f,
            lang,
            library_title,
            relative_folder_path,
            inheritance,
        ),
        Modal::ConfirmApplyStaging { note } => draw_confirm_apply_modal(f, app, note),
        Modal::ConfirmRevertTransaction {
            tx_id,
            only_op_id,
            summary,
        } => draw_confirm_revert_modal(f, lang, *tx_id, *only_op_id, summary),
        Modal::SyncControlMenu {
            scope,
            selected_idx,
        } => draw_sync_control_modal(f, app, *scope, *selected_idx),
        Modal::ConfirmDestructiveClone {
            source_url,
            target_url,
            typed_confirm,
        } => draw_confirm_destructive_clone_modal(f, lang, source_url, target_url, typed_confirm),
    }
}

fn draw_help_modal(f: &mut Frame<'_>, lang: Lang) {
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
            Line::from(Span::styled(
                "Pulsa Esc o ? para cerrar esta ayuda",
                Style::default().fg(Color::Yellow),
            )),
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
            Line::from(Span::styled(
                "Press Esc or ? to close this help",
                Style::default().fg(Color::Yellow),
            )),
        ],
    };
    let block = Block::default()
        .title(lang.tr(" Ayuda de SPOIberia ", " SPOIberia Help "))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Yellow));
    f.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: false }), area);
}

fn draw_text_input_modal(f: &mut Frame<'_>, lang: Lang, title: &str, prompt: &str, value: &str) {
    let area = centered_rect(65, 28, f.area());
    f.render_widget(Clear, area);
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(prompt, Style::default().fg(Color::Cyan))),
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

#[allow(clippy::too_many_arguments)]
fn draw_add_user_modal(
    f: &mut Frame<'_>,
    app: &App,
    group_title: &str,
    query: &str,
    results: &[DirectoryUser],
    selected_idx: usize,
    marked_identifiers: &HashSet<String>,
    searching: bool,
) {
    let lang = app.lang;
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

    let status = if searching {
        lang.tr(" (Buscando...)", " (Searching...)")
    } else {
        ""
    };
    let search_block = Block::default()
        .title(match lang {
            Lang::Es => format!(" Asignar Usuario(s) al Grupo '{}'{} ", group_title, status),
            Lang::En => format!(" Add User(s) to Group '{}'{} ", group_title, status),
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
            let is_sel = idx == selected_idx;
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
        selected_idx,
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

#[allow(clippy::too_many_arguments)]
fn draw_grant_permission_modal(
    f: &mut Frame<'_>,
    lang: Lang,
    library_title: &str,
    relative_folder_path: &str,
    target_has_unique_roles: bool,
    target_current_roles: &[FolderPermissionEntry],
    inheritance: &FolderInheritanceSummary,
    group_name: &str,
    role_idx: usize,
    copy_parent_roles: bool,
    replace_inherited_roles: bool,
    clear_subscopes: bool,
    create_group_if_missing: bool,
    active_field: usize,
) {
    let area = centered_rect(82, 78, f.area());
    f.render_widget(Clear, area);
    let role_name = STANDARD_ROLES.get(role_idx).copied().unwrap_or("Read");
    let impact = analyze_permission_change_impact(
        target_has_unique_roles,
        target_current_roles,
        inheritance,
        group_name,
        role_name,
        copy_parent_roles,
        clear_subscopes,
    );

    let field_style = |idx: usize| -> Style {
        if active_field == idx {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        }
    };

    let yes = lang.tr("Sí", "Yes");
    let no = "No";
    let state_badge = if target_has_unique_roles {
        lang.tr(
            "Permisos ÚNICOS (herencia ya rota)",
            "UNIQUE permissions (inheritance already broken)",
        )
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
                Style::default().fg(if target_has_unique_roles {
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
                    if copy_parent_roles { yes } else { no }
                ),
                Lang::En => format!(
                    "  3. CopyParentRoles (copy inherited roles on break):       [{}]",
                    if copy_parent_roles { yes } else { no }
                ),
            },
            field_style(2),
        )),
        Line::from(Span::styled(
            match lang {
                Lang::Es => format!(
                    "  4. Reemplazar rol previo del mismo grupo (evita suma):     [{}]",
                    if replace_inherited_roles { yes } else { no }
                ),
                Lang::En => format!(
                    "  4. Replace existing role for same group (avoid union):     [{}]",
                    if replace_inherited_roles { yes } else { no }
                ),
            },
            field_style(3),
        )),
        Line::from(Span::styled(
            match lang {
                Lang::Es => format!(
                    "  5. ClearSubscopes (resetear permisos únicos en subcarpetas):[{}]",
                    if clear_subscopes { yes } else { no }
                ),
                Lang::En => format!(
                    "  5. ClearSubscopes (wipe unique roles on child folders):    [{}]",
                    if clear_subscopes { yes } else { no }
                ),
            },
            field_style(4),
        )),
        Line::from(""),
        Line::from(Span::styled(
            lang.tr("  ── Impacto en herencias ──", "  ── Inheritance Impact ──"),
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
        if replace_inherited_roles {
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

#[allow(clippy::too_many_arguments)]
fn draw_manage_folder_permissions_modal(
    f: &mut Frame<'_>,
    app: &App,
    library_title: &str,
    relative_folder_path: &str,
    target_has_unique_roles: bool,
    inheritance: &FolderInheritanceSummary,
    entries: &[(String, String, String)],
    selected_idx: usize,
) {
    let lang = app.lang;
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

    let note_line = if target_has_unique_roles {
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
            let is_sel = idx == selected_idx;
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
        selected_idx,
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

fn draw_confirm_reset_inheritance_modal(
    f: &mut Frame<'_>,
    lang: Lang,
    library_title: &str,
    relative_folder_path: &str,
    inheritance: &FolderInheritanceSummary,
) {
    let area = centered_rect(76, 46, f.area());
    f.render_widget(Clear, area);
    let inherited_roles_str = if inheritance.inherited_roles.is_empty() {
        lang.tr("(los roles actuales del padre)", "(current parent roles)")
            .to_string()
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
                Lang::Es => format!("  Carpeta: {}/{}", library_title, relative_folder_path),
                Lang::En => format!("  Folder:  {}/{}", library_title, relative_folder_path),
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

fn draw_confirm_apply_modal(f: &mut Frame<'_>, app: &App, note: &str) {
    let lang = app.lang;
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

fn draw_confirm_revert_modal(
    f: &mut Frame<'_>,
    lang: Lang,
    tx_id: i64,
    only_op_id: Option<i64>,
    summary: &str,
) {
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

fn draw_sync_control_modal(
    f: &mut Frame<'_>,
    app: &App,
    scope: SyncMenuScope,
    selected_idx: usize,
) {
    let lang = app.lang;
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
                    Lang::Es => format!(
                        " Control Sync / Accesos OneDrive — Biblioteca '{}' ",
                        lib_name
                    ),
                    Lang::En => format!(
                        " Sync / OneDrive Shortcuts Control — Library '{}' ",
                        lib_name
                    ),
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
            let both_blocked = sync_eff && (app.libraries.is_empty() || od_eff);
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
        let style = if idx == selected_idx {
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

fn draw_confirm_destructive_clone_modal(
    f: &mut Frame<'_>,
    lang: Lang,
    source_url: &str,
    target_url: &str,
    typed_confirm: &str,
) {
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
