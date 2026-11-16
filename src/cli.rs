use crate::api::graph::GraphClient;
use crate::api::spo::SpoClient;
use crate::auth::AuthManager;
use crate::cloner::SiteClonerEngine;
use crate::db::Database;
use crate::importer::MatrixImporter;
use crate::models::{CloneConfig, StagingOp};
use crate::staging::{StagingQueue, TransactionExecutor};
use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use serde_json::json;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Table,
    Json,
    Csv,
}

#[derive(Debug, Parser)]
#[command(
    name = "spoiberia",
    version,
    about = "SPOIberia — Gestión de Autorizaciones, Aprovisionamiento de Carpetas y Clonado en SharePoint Online (TUI + Agent CLI)"
)]
pub struct Cli {
    /// Formato de salida en modo comando (table, json, csv)
    #[arg(short = 'o', long = "output", global = true, default_value = "table")]
    pub output: OutputFormat,

    /// Atajo para `--output json` (ideal para agentes autónomos)
    #[arg(long = "json", global = true)]
    pub json: bool,

    /// Modo no interactivo (nunca bloquea esperando prompts)
    #[arg(long = "non-interactive", global = true)]
    pub non_interactive: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

impl Cli {
    pub fn effective_format(&self) -> OutputFormat {
        if self.json {
            OutputFormat::Json
        } else {
            self.output
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Inicia la interfaz interactiva en terminal (TUI) [Por defecto si no se indica subcomando]
    Tui,

    /// Gestión de autenticación OAuth2 contra Microsoft Entra ID (M365)
    Auth {
        #[command(subcommand)]
        action: AuthCommand,
    },

    /// Descubrimiento y verificación de sitios SharePoint donde eres Administrador o Propietario
    Sites {
        #[command(subcommand)]
        action: SitesCommand,
    },

    /// Consulta y creación de grupos de SharePoint de un sitio
    Groups {
        #[command(subcommand)]
        action: GroupsCommand,
    },

    /// Búsqueda de usuarios en el directorio M365 / SharePoint People Picker
    Users {
        #[command(subcommand)]
        action: UsersCommand,
    },

    /// Asignar o remover usuarios en grupos de SharePoint (registra transacción reversible en SQLite)
    Members {
        #[command(subcommand)]
        action: MembersCommand,
    },

    /// Historial de transacciones en SQLite y reversión (Rollback)
    Tx {
        #[command(subcommand)]
        action: TxCommand,
    },

    /// (Fase 2) Gestión de bibliotecas de documentos del sitio
    Libraries {
        #[command(subcommand)]
        action: LibrariesCommand,
    },

    /// (Fase 2) Creación de carpetas y asignación de permisos únicos por carpeta
    Folders {
        #[command(subcommand)]
        action: FoldersCommand,
    },

    /// (Fase 2) Importación y exportación de matriz de carpetas y permisos desde fichero externo (CSV / JSON)
    Matrix {
        #[command(subcommand)]
        action: MatrixCommand,
    },

    /// (Fase 3) Clonar completamente un sitio SharePoint Origen en un sitio Destino (grupos, datos server-side y permisos)
    Clone {
        /// URL del sitio SharePoint Origen
        #[arg(long = "source-site")]
        source_site: String,

        /// URL del sitio SharePoint Destino
        #[arg(long = "target-site")]
        target_site: String,

        /// Bibliotecas a clonar separadas por coma (o "all" para todas)
        #[arg(long = "libraries", default_value = "all")]
        libraries: String,

        /// Carpeta base opcional dentro de la biblioteca destino
        #[arg(long = "target-base-folder", default_value = "")]
        target_base_folder: String,

        /// Omitir clonado de grupos y miembros del sitio
        #[arg(long = "skip-groups")]
        skip_groups: bool,

        /// Omitir copia Server-Side de carpetas y archivos
        #[arg(long = "skip-files")]
        skip_files: bool,

        /// Omitir replicación de permisos únicos en carpetas
        #[arg(long = "skip-unique-permissions")]
        skip_unique_permissions: bool,

        /// Sobreescribir archivos existentes en el destino
        #[arg(long = "overwrite")]
        overwrite: bool,

        /// Nota descriptiva para la transacción en SQLite
        #[arg(long = "note")]
        note: Option<String>,

        /// Previsualizar el plan de clonado sin aplicarlo
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Muestra el estado de la sesión actual
    Status,
    /// Inicia sesión en Microsoft 365 (Browser PKCE por defecto o --device-code)
    Login {
        #[arg(long = "device-code")]
        device_code: bool,
        #[arg(long = "tenant")]
        tenant: Option<String>,
        #[arg(long = "client-id")]
        client_id: Option<String>,
    },
    /// Cierra la sesión actual y elimina los tokens cacheados
    Logout,
}

#[derive(Debug, Subcommand)]
pub enum SitesCommand {
    /// Lista los sitios SharePoint donde eres Administrador o Propietario
    List {
        /// Filtro o palabra clave de búsqueda en el tenant
        #[arg(short = 's', long = "search")]
        search: Option<String>,
        /// Filtrar por rol: all, admin, owner
        #[arg(long = "role", default_value = "all")]
        role: String,
        /// Mostrar primero la caché local de SQLite sin consultar la red
        #[arg(long = "cached")]
        cached: bool,
    },
    /// Verifica el rol del usuario actual en un sitio SharePoint específico por URL
    Check {
        #[arg(long = "site")]
        site: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum GroupsCommand {
    /// Lista los grupos de SharePoint de un sitio y sus niveles de permiso
    List {
        #[arg(long = "site")]
        site: String,
    },
    /// Lista los usuarios miembros de un grupo de SharePoint
    Members {
        #[arg(long = "site")]
        site: String,
        /// ID numérico o nombre exacto del grupo
        #[arg(long = "group")]
        group: String,
    },
    /// Crea un nuevo grupo de SharePoint en el sitio
    Create {
        #[arg(long = "site")]
        site: String,
        #[arg(long = "title")]
        title: String,
        #[arg(long = "description", default_value = "")]
        description: String,
        #[arg(long = "note")]
        note: Option<String>,
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum UsersCommand {
    /// Busca usuarios en el directorio M365 y SharePoint People Picker
    Search {
        #[arg(short = 'q', long = "query")]
        query: String,
        #[arg(long = "site")]
        site: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum MembersCommand {
    /// Asigna uno o varios usuarios (separados por coma) a un grupo de SharePoint
    Add {
        #[arg(long = "site")]
        site: String,
        #[arg(long = "group")]
        group: String,
        /// Emails o UPNs separados por coma
        #[arg(long = "users")]
        users: String,
        #[arg(long = "note")]
        note: Option<String>,
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
    /// Remueve uno o varios usuarios (separados por coma) de un grupo de SharePoint
    Remove {
        #[arg(long = "site")]
        site: String,
        #[arg(long = "group")]
        group: String,
        /// Emails o UPNs separados por coma
        #[arg(long = "users")]
        users: String,
        #[arg(long = "note")]
        note: Option<String>,
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum TxCommand {
    /// Lista las transacciones registradas en SQLite
    List {
        #[arg(long = "site")]
        site: Option<String>,
        #[arg(long = "limit", default_value_t = 30)]
        limit: usize,
    },
    /// Muestra el detalle y las operaciones individuales de una transacción
    Show {
        /// ID de la transacción en SQLite
        tx_id: i64,
    },
    /// Revierte (Rollback) una transacción completa o una operación individual en orden inverso LIFO
    Revert {
        /// ID de la transacción en SQLite
        tx_id: i64,
        /// Revertir únicamente esta operación dentro de la transacción
        #[arg(long = "op-id")]
        op_id: Option<i64>,
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum LibrariesCommand {
    /// Lista las bibliotecas de documentos del sitio
    List {
        #[arg(long = "site")]
        site: String,
    },
    /// Crea una nueva biblioteca de documentos en el sitio
    Create {
        #[arg(long = "site")]
        site: String,
        #[arg(long = "title")]
        title: String,
        #[arg(long = "description", default_value = "")]
        description: String,
        #[arg(long = "note")]
        note: Option<String>,
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum FoldersCommand {
    /// Lista las carpetas dentro de una biblioteca (y sus permisos únicos si tienen herencia rota)
    List {
        #[arg(long = "site")]
        site: String,
        #[arg(long = "library", default_value = "Documents")]
        library: String,
        #[arg(long = "path")]
        path: Option<String>,
    },
    /// Crea una carpeta (o ruta de subcarpetas) dentro de una biblioteca
    Create {
        #[arg(long = "site")]
        site: String,
        #[arg(long = "library", default_value = "Documents")]
        library: String,
        #[arg(long = "path")]
        path: String,
        #[arg(long = "note")]
        note: Option<String>,
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
    /// Rompe herencia y asigna un nivel de permiso a un grupo/usuario sobre una carpeta
    Grant {
        #[arg(long = "site")]
        site: String,
        #[arg(long = "library", default_value = "Documents")]
        library: String,
        #[arg(long = "path")]
        path: String,
        #[arg(long = "group")]
        group: String,
        #[arg(long = "role", default_value = "Read")]
        role: String,
        #[arg(long = "principal-type", default_value = "SharePointGroup")]
        principal_type: String,
        #[arg(long = "create-group", default_value_t = true)]
        create_group: bool,
        #[arg(long = "copy-parent-roles", default_value_t = false)]
        copy_parent_roles: bool,
        #[arg(long = "clear-subscopes", default_value_t = false)]
        clear_subscopes: bool,
        #[arg(long = "note")]
        note: Option<String>,
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum MatrixCommand {
    /// Carga y aplica una matriz de carpetas y permisos desde un archivo externo (.csv o .json)
    Apply {
        #[arg(long = "site")]
        site: String,
        #[arg(long = "file")]
        file: PathBuf,
        #[arg(long = "library")]
        library: Option<String>,
        #[arg(long = "note")]
        note: Option<String>,
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
    /// Exporta la matriz actual de carpetas con permisos únicos de una biblioteca a .csv o .json
    Export {
        #[arg(long = "site")]
        site: String,
        #[arg(long = "library", default_value = "Documents")]
        library: String,
        #[arg(long = "out")]
        out: PathBuf,
    },
}

pub async fn run_cli_command(cli: &Cli, cmd: &Commands) -> Result<()> {
    let format = cli.effective_format();
    let auth = AuthManager::new()?;
    let db = Database::open_default()?;
    let graph = GraphClient::new(auth.clone())?;
    let spo = SpoClient::new(auth.clone())?;
    let executor = TransactionExecutor::new(spo.clone(), db.clone());

    let actor_upn = auth
        .load_session()?
        .and_then(|s| s.user_upn)
        .unwrap_or_else(|| "cli-agent".to_string());

    match cmd {
        Commands::Tui => unreachable!(),
        Commands::Auth { action } => match action {
            AuthCommand::Status => {
                if let Some(sess) = auth.load_session()? {
                    let payload = json!({
                        "authenticated": true,
                        "tenant_id": sess.tenant_id,
                        "client_id": sess.client_id,
                        "user_upn": sess.user_upn,
                        "user_display_name": sess.user_display_name,
                        "root_site_url": sess.root_site_url,
                        "cached_resources": sess.tokens.keys().collect::<Vec<_>>(),
                    });
                    print_output(&payload, format, || {
                        println!("Estado: AUTENTICADO");
                        println!("Usuario: {} ({})", sess.user_display_name.unwrap_or_default(), sess.user_upn.unwrap_or_default());
                        println!("Tenant:  {}", sess.tenant_id);
                        println!("Root SPO: {}", sess.root_site_url.unwrap_or_default());
                    })?;
                } else {
                    let payload = json!({ "authenticated": false });
                    print_output(&payload, format, || {
                        println!("Estado: NO AUTENTICADO (ejecuta `spoiberia auth login`)");
                    })?;
                }
            }
            AuthCommand::Login {
                device_code,
                tenant,
                client_id,
            } => {
                let sess = if *device_code {
                    let (prompt, t, c) = auth
                        .start_device_code(tenant.as_deref(), client_id.as_deref())
                        .await?;
                    eprintln!("{}", prompt.message);
                    auth.poll_device_code(&prompt, &t, &c).await?
                } else {
                    eprintln!("Abriendo el navegador para iniciar sesión en Microsoft 365...");
                    auth.login_interactive_browser(tenant.as_deref(), client_id.as_deref())
                        .await?
                };
                let (me, root) = graph.get_me_and_root_site().await?;
                let payload = json!({
                    "status": "AUTHENTICATED",
                    "tenant_id": sess.tenant_id,
                    "user_upn": me.user_principal_name.or(me.mail),
                    "display_name": me.display_name,
                    "root_site_url": root.web_url,
                });
                print_output(&payload, format, || {
                    println!("✓ Sesión iniciada correctamente como {} ({})", payload["display_name"], payload["user_upn"]);
                })?;
            }
            AuthCommand::Logout => {
                auth.logout()?;
                let payload = json!({ "status": "LOGGED_OUT" });
                print_output(&payload, format, || {
                    println!("✓ Sesión cerrada correctamente.");
                })?;
            }
        },
        Commands::Sites { action } => match action {
            SitesCommand::List {
                search,
                role,
                cached,
            } => {
                let mut sites = if *cached {
                    db.load_cached_sites()?
                } else {
                    let discovered = spo
                        .discover_admin_and_owner_sites(&graph, search.as_deref())
                        .await?;
                    let _ = db.save_cached_sites(&discovered);
                    discovered
                };
                sites.retain(|s| s.role.matches_filter(role));
                print_output(&sites, format, || {
                    println!("{:<14} | {:<38} | URL", "ROL", "TÍTULO");
                    println!("{}", "-".repeat(95));
                    for s in &sites {
                        println!("{:<14} | {:<38} | {}", s.role.badge(), truncate(&s.title, 38), s.url);
                    }
                })?;
            }
            SitesCommand::Check { site } => {
                let checked = spo.check_user_site_role(site).await?;
                print_output(&checked, format, || match &checked {
                    Some(s) => println!("✓ Tienes acceso de {} en '{}' ({})", s.role.badge(), s.title, s.url),
                    None => println!("✗ No eres Administrador ni Propietario en '{}'", site),
                })?;
            }
        },
        Commands::Groups { action } => match action {
            GroupsCommand::List { site } => {
                let groups = spo.get_site_groups(site).await?;
                print_output(&groups, format, || {
                    println!("{:<6} | {:<10} | {:<36} | PERMISOS EN SITIO", "ID", "TIPO", "NOMBRE DEL GRUPO");
                    println!("{}", "-".repeat(95));
                    for g in &groups {
                        println!(
                            "{:<6} | {:<10} | {:<36} | {}",
                            g.id,
                            g.kind.badge(),
                            truncate(&g.title, 36),
                            g.permission_levels.join(", ")
                        );
                    }
                })?;
            }
            GroupsCommand::Members { site, group } => {
                let grp = spo.resolve_group(site, group).await?;
                let members = spo.get_group_members(site, grp.id).await?;
                print_output(&members, format, || {
                    println!("Miembros de '{}' (Id: {}):", grp.title, grp.id);
                    println!("{:<6} | {:<32} | {:<38} | ADMIN", "ID", "NOMBRE", "EMAIL / UPN");
                    println!("{}", "-".repeat(90));
                    for m in &members {
                        println!(
                            "{:<6} | {:<32} | {:<38} | {}",
                            m.id,
                            truncate(&m.title, 32),
                            truncate(&m.clean_identifier(), 38),
                            if m.is_site_admin { "Sí" } else { "No" }
                        );
                    }
                })?;
            }
            GroupsCommand::Create {
                site,
                title,
                description,
                note,
                dry_run,
            } => {
                let mut q = StagingQueue::new();
                q.push(StagingOp::CreateGroup {
                    site_url: site.clone(),
                    group_title: title.clone(),
                    description: description.clone(),
                });
                let report = executor
                    .apply_queue(&q, &actor_upn, site, site, "CLI", note.as_deref(), *dry_run)
                    .await?;
                print_execution_report(&report, format)?;
            }
        },
        Commands::Users { action } => match action {
            UsersCommand::Search { query, site } => {
                let mut users = graph.search_directory_users(query).await.unwrap_or_default();
                if users.is_empty() {
                    if let Some(s_url) = site {
                        users = spo.search_people_picker(s_url, query).await.unwrap_or_default();
                    }
                }
                print_output(&users, format, || {
                    println!("{:<32} | {:<38} | DEPARTAMENTO / PUESTO", "NOMBRE", "UPN / EMAIL");
                    println!("{}", "-".repeat(95));
                    for u in &users {
                        let meta = format!(
                            "{} / {}",
                            u.department.as_deref().unwrap_or("-"),
                            u.job_title.as_deref().unwrap_or("-")
                        );
                        println!("{:<32} | {:<38} | {}", truncate(&u.display_name, 32), truncate(u.identifier(), 38), meta);
                    }
                })?;
            }
        },
        Commands::Members { action } => match action {
            MembersCommand::Add {
                site,
                group,
                users,
                note,
                dry_run,
            } => {
                let grp = spo.resolve_group(site, group).await?;
                let mut q = StagingQueue::new();
                for u in users.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    q.push(StagingOp::AddUserToGroup {
                        site_url: site.clone(),
                        group_id: grp.id,
                        group_title: grp.title.clone(),
                        user_identifier: u.to_string(),
                        user_display_name: u.to_string(),
                    });
                }
                let report = executor
                    .apply_queue(&q, &actor_upn, site, site, "CLI", note.as_deref(), *dry_run)
                    .await?;
                print_execution_report(&report, format)?;
            }
            MembersCommand::Remove {
                site,
                group,
                users,
                note,
                dry_run,
            } => {
                let grp = spo.resolve_group(site, group).await?;
                let mut q = StagingQueue::new();
                for u in users.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    q.push(StagingOp::RemoveUserFromGroup {
                        site_url: site.clone(),
                        group_id: grp.id,
                        group_title: grp.title.clone(),
                        user_id: None,
                        user_identifier: u.to_string(),
                        user_display_name: u.to_string(),
                    });
                }
                let report = executor
                    .apply_queue(&q, &actor_upn, site, site, "CLI", note.as_deref(), *dry_run)
                    .await?;
                print_execution_report(&report, format)?;
            }
        },
        Commands::Tx { action } => match action {
            TxCommand::List { site, limit } => {
                let txs = db.list_transactions(site.as_deref(), *limit)?;
                print_output(&txs, format, || {
                    println!("{:<6} | {:<20} | {:<10} | {:<12} | RESUMEN", "ID", "FECHA (UTC)", "ESTADO", "ORIGEN");
                    println!("{}", "-".repeat(95));
                    for t in &txs {
                        let short_date = t.created_at.chars().take(19).collect::<String>();
                        println!(
                            "#{:<5} | {:<20} | {:<10} | {:<12} | {} ({})",
                            t.id, short_date, t.status, truncate(&t.source, 12), t.summary, t.site_url
                        );
                    }
                })?;
            }
            TxCommand::Show { tx_id } => {
                let tx = db.get_transaction(*tx_id)?;
                let ops = db.get_transaction_operations(*tx_id)?;
                let payload = json!({
                    "transaction": tx,
                    "operations": ops,
                });
                print_output(&payload, format, || {
                    if let Some(ref t) = tx {
                        println!("Transacción #{} [{}] — {}", t.id, t.status, t.summary);
                        println!("Sitio: {} | Autor: {} | Fecha: {}", t.site_url, t.actor_upn, t.created_at);
                        if let Some(ref n) = t.note {
                            println!("Nota:  {}", n);
                        }
                        println!("{}", "-".repeat(90));
                        for op in &ops {
                            println!(
                                "  Op #{:<4} (seq {:>2}) [{:<8}] {} — {}",
                                op.id, op.seq_order, op.status, op.op_type, op.target_label
                            );
                        }
                    } else {
                        println!("No existe la transacción #{}", tx_id);
                    }
                })?;
            }
            TxCommand::Revert {
                tx_id,
                op_id,
                dry_run,
            } => {
                let (updated_tx, ops_summary) =
                    executor.revert_transaction(*tx_id, *op_id, *dry_run).await?;
                let payload = json!({
                    "transaction": updated_tx,
                    "reverted_operations": ops_summary,
                    "dry_run": dry_run,
                });
                print_output(&payload, format, || {
                    println!(
                        "Resultado Rollback Transacción #{} -> Estado: {}",
                        updated_tx.id, updated_tx.status
                    );
                    for s in &ops_summary {
                        println!("  [{:<14}] {}", s.status, s.target);
                    }
                })?;
            }
        },
        Commands::Libraries { action } => match action {
            LibrariesCommand::List { site } => {
                let libs = spo.get_document_libraries(site).await?;
                print_output(&libs, format, || {
                    println!("{:<28} | {:<8} | {:<10} | RUTA RELATIVA", "TÍTULO", "ITEMS", "DEFAULT");
                    println!("{}", "-".repeat(90));
                    for l in &libs {
                        println!(
                            "{:<28} | {:<8} | {:<10} | {}",
                            truncate(&l.title, 28),
                            l.item_count,
                            if l.is_default_documents { "Sí" } else { "No" },
                            l.server_relative_url
                        );
                    }
                })?;
            }
            LibrariesCommand::Create {
                site,
                title,
                description,
                note,
                dry_run,
            } => {
                let mut q = StagingQueue::new();
                q.push(StagingOp::CreateLibrary {
                    site_url: site.clone(),
                    library_title: title.clone(),
                    description: description.clone(),
                });
                let report = executor
                    .apply_queue(&q, &actor_upn, site, site, "CLI", note.as_deref(), *dry_run)
                    .await?;
                print_execution_report(&report, format)?;
            }
        },
        Commands::Folders { action } => match action {
            FoldersCommand::List {
                site,
                library,
                path,
            } => {
                let folders = spo.list_folders(site, library, path.as_deref()).await?;
                print_output(&folders, format, || {
                    println!("{:<34} | {:<14} | PERMISOS ASIGNADOS", "CARPETA (RUTA RELATIVA)", "HERENCIA ROTA");
                    println!("{}", "-".repeat(95));
                    for f in &folders {
                        let perms_str = if f.role_assignments.is_empty() {
                            "(Hereda del padre)".to_string()
                        } else {
                            f.role_assignments
                                .iter()
                                .map(|r| format!("{}: [{}]", r.principal_title, r.roles.join(",")))
                                .collect::<Vec<_>>()
                                .join("; ")
                        };
                        println!(
                            "{:<34} | {:<14} | {}",
                            truncate(&f.relative_path, 34),
                            if f.has_unique_role_assignments { "Sí (Únicos)" } else { "No" },
                            perms_str
                        );
                    }
                })?;
            }
            FoldersCommand::Create {
                site,
                library,
                path,
                note,
                dry_run,
            } => {
                let mut q = StagingQueue::new();
                q.push(StagingOp::CreateFolder {
                    site_url: site.clone(),
                    library_title: library.clone(),
                    relative_folder_path: path.clone(),
                });
                let report = executor
                    .apply_queue(&q, &actor_upn, site, site, "CLI", note.as_deref(), *dry_run)
                    .await?;
                print_execution_report(&report, format)?;
            }
            FoldersCommand::Grant {
                site,
                library,
                path,
                group,
                role,
                principal_type,
                create_group,
                copy_parent_roles,
                clear_subscopes,
                note,
                dry_run,
            } => {
                let mut q = StagingQueue::new();
                if *create_group && principal_type.eq_ignore_ascii_case("SharePointGroup") {
                    q.push(StagingOp::CreateGroup {
                        site_url: site.clone(),
                        group_title: group.clone(),
                        description: format!("Grupo creado para carpeta {}", path),
                    });
                }
                q.push(StagingOp::CreateFolder {
                    site_url: site.clone(),
                    library_title: library.clone(),
                    relative_folder_path: path.clone(),
                });
                q.push(StagingOp::BreakFolderInheritance {
                    site_url: site.clone(),
                    library_title: library.clone(),
                    relative_folder_path: path.clone(),
                    copy_parent_roles: *copy_parent_roles,
                    clear_subscopes: *clear_subscopes,
                });
                q.push(StagingOp::AssignFolderRole {
                    site_url: site.clone(),
                    library_title: library.clone(),
                    relative_folder_path: path.clone(),
                    principal_name: group.clone(),
                    principal_type: principal_type.clone(),
                    role_name: role.clone(),
                    create_group_if_missing: *create_group,
                });

                let report = executor
                    .apply_queue(&q, &actor_upn, site, site, "CLI", note.as_deref(), *dry_run)
                    .await?;
                print_execution_report(&report, format)?;
            }
        },
        Commands::Matrix { action } => match action {
            MatrixCommand::Apply {
                site,
                file,
                library,
                note,
                dry_run,
            } => {
                let specs = MatrixImporter::load_from_file(file, library.as_deref())?;
                let q = MatrixImporter::specs_to_staging_queue(site, &specs);
                let src_tag = format!(
                    "MATRIX_IMPORT:{}",
                    file.file_name().and_then(|n| n.to_str()).unwrap_or("file")
                );
                let report = executor
                    .apply_queue(&q, &actor_upn, site, site, &src_tag, note.as_deref(), *dry_run)
                    .await?;
                print_execution_report(&report, format)?;
            }
            MatrixCommand::Export { site, library, out } => {
                let count =
                    MatrixImporter::export_library_permissions_to_file(&spo, site, library, out)
                        .await?;
                let payload = json!({
                    "status": "EXPORTED",
                    "site_url": site,
                    "library": library,
                    "folders_with_unique_permissions": count,
                    "output_file": out.display().to_string(),
                });
                print_output(&payload, format, || {
                    println!(
                        "✓ Exportadas {} carpetas con permisos únicos a '{}'",
                        count,
                        out.display()
                    );
                })?;
            }
        },
        Commands::Clone {
            source_site,
            target_site,
            libraries,
            target_base_folder,
            skip_groups,
            skip_files,
            skip_unique_permissions,
            overwrite,
            note,
            dry_run,
        } => {
            let libs_filter = if libraries.trim().eq_ignore_ascii_case("all") {
                None
            } else {
                Some(
                    libraries
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect(),
                )
            };

            let cfg = CloneConfig {
                source_site_url: source_site.clone(),
                target_site_url: target_site.clone(),
                libraries_filter: libs_filter,
                target_base_folder: target_base_folder.clone(),
                include_groups: !skip_groups,
                include_files: !skip_files,
                include_unique_permissions: !skip_unique_permissions,
                overwrite: *overwrite,
                note: note.clone(),
            };

            let cloner = SiteClonerEngine::new(spo.clone());
            let q = cloner.plan_clone(&cfg).await?;
            let src_tag = format!("SITE_CLONE:{}", source_site);
            let report = executor
                .apply_queue(
                    &q,
                    &actor_upn,
                    target_site,
                    target_site,
                    &src_tag,
                    note.as_deref(),
                    *dry_run,
                )
                .await?;
            print_execution_report(&report, format)?;
        }
    }

    Ok(())
}

fn print_execution_report(
    report: &crate::staging::ExecutionReport,
    format: OutputFormat,
) -> Result<()> {
    print_output(report, format, || {
        println!(
            "Estado: {} | Sitio: {} | Transacción: {:?}",
            report.status, report.site_url, report.transaction_id
        );
        println!("Resumen: {}", report.summary);
        println!("{}", "-".repeat(90));
        for op in &report.operations {
            println!("  {:>2}. [{:<8}] {}", op.seq_order, op.status, op.target);
            if let Some(ref err) = op.error {
                println!("      Error: {}", err);
            }
        }
    })
}

fn print_output<T: Serialize, F: FnOnce()>(
    data: &T,
    format: OutputFormat,
    table_printer: F,
) -> Result<()> {
    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(data)?);
        }
        OutputFormat::Csv => {
            println!("{}", serde_json::to_string(data)?);
        }
        OutputFormat::Table => {
            table_printer();
        }
    }
    Ok(())
}

fn truncate(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn verify_cli_structure() {
        Cli::command().debug_assert();
    }
}
