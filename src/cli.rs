use crate::api::graph::GraphClient;
use crate::api::spo::SpoClient;
use crate::auth::AuthManager;
use crate::cloner::SiteClonerEngine;
use crate::db::Database;
use crate::importer::MatrixImporter;
use crate::models::{CloneConfig, CloneMode, SiteSyncStatus, StagingOp};
use crate::staging::{ExecutionReport, StagingQueue, TransactionExecutor};
use anyhow::Result;
use clap::builder::styling::{AnsiColor, Effects, Styles};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CloneModeArg {
    /// Mantiene el contenido existente en destino y añade/sobreescribe lo del origen (por defecto)
    #[value(alias = "aditivo")]
    Additive,
    /// DESTRUCTIVO: elimina bibliotecas, carpetas, archivos, grupos y usuarios existentes en destino antes de clonar
    #[value(alias = "destructivo")]
    Destructive,
}

/// Color palette for CLI help and error output across all commands and subcommands.
pub fn cli_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default() | Effects::BOLD | Effects::UNDERLINE)
        .usage(AnsiColor::Yellow.on_default() | Effects::BOLD | Effects::UNDERLINE)
        .literal(AnsiColor::Cyan.on_default() | Effects::BOLD)
        .placeholder(AnsiColor::Green.on_default())
        .valid(AnsiColor::Green.on_default() | Effects::BOLD)
        .invalid(AnsiColor::Yellow.on_default() | Effects::BOLD)
        .error(AnsiColor::Red.on_default() | Effects::BOLD)
}

#[derive(Debug, Parser)]
#[command(
    name = "spoiberia",
    version,
    styles = cli_styles(),
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

    /// Idioma de la interfaz TUI (`auto`, `es`, `en`) / TUI language (`auto`, `es`, `en`)
    #[arg(
        long = "lang",
        global = true,
        env = "SPOIBERIA_LANG",
        value_enum,
        default_value_t = crate::i18n::LangArg::Auto
    )]
    pub lang: crate::i18n::LangArg,

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

        /// Modo de clonado: `additive` (por defecto) o `destructive` (BORRA contenido y grupos previos del destino)
        #[arg(long = "mode", value_enum, default_value_t = CloneModeArg::Additive)]
        mode: CloneModeArg,

        /// Confirmación explícita requerida para ejecutar `--mode destructive` sin `--dry-run`
        #[arg(long = "confirm-destructive")]
        confirm_destructive: bool,

        /// Nota descriptiva para la transacción en SQLite
        #[arg(long = "note")]
        note: Option<String>,

        /// Previsualizar el plan de clonado sin aplicarlo
        #[arg(long = "dry-run")]
        dry_run: bool,
    },

    /// (Fase 4) Consultar, activar o desactivar la sincronización offline (OneDrive Sync / Accesos directos)
    Sync {
        #[command(subcommand)]
        action: SyncCommand,
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
        #[arg(short = 'u', long = "email", visible_alias = "user")]
        email: Option<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SyncTarget {
    /// Ambos: sincronización offline de SharePoint y accesos directos a OneDrive
    All,
    /// Solo sincronización offline de SharePoint (`ExcludeFromOfflineClient`)
    #[value(alias = "offline", alias = "spo")]
    Sharepoint,
    /// Solo botón "Agregar acceso directo a OneDrive" (`CustomFormatter`)
    #[value(alias = "shortcuts", alias = "accesos")]
    Onedrive,
}

#[derive(Debug, Subcommand)]
pub enum SyncCommand {
    /// Consulta el estado de sincronización offline (`ExcludeFromOfflineClient`) y botón OneDrive del sitio y sus bibliotecas
    Status {
        #[arg(long = "site")]
        site: String,
        /// Incluir subsitios de forma recursiva
        #[arg(long = "include-subsites")]
        include_subsites: bool,
    },
    /// Gestiona únicamente el bloqueo de sincronización offline de SharePoint (`ExcludeFromOfflineClient`)
    #[command(visible_alias = "offline")]
    Sharepoint {
        #[command(subcommand)]
        action: SharepointSyncCommand,
    },
    /// Gestiona únicamente el botón "Agregar acceso directo a OneDrive" (`CustomFormatter`)
    #[command(visible_alias = "shortcuts")]
    Onedrive {
        #[command(subcommand)]
        action: OneDriveShortcutsCommand,
    },
    /// Desactiva la sincronización offline de SharePoint y/o los accesos directos a OneDrive
    Disable {
        #[arg(long = "site")]
        site: String,
        /// Nombre(s) de biblioteca separados por coma (si se omite, aplica al sitio y todas sus bibliotecas)
        #[arg(long = "library", conflicts_with_all = ["site_only", "libraries_only", "include_subsites"])]
        library: Option<String>,
        /// Aplicar únicamente a nivel de sitio (`SP.Web`), sin modificar bibliotecas individuales
        #[arg(long = "site-only", conflicts_with = "libraries_only")]
        site_only: bool,
        /// Aplicar únicamente a las bibliotecas de documentos, sin modificar el sitio (`SP.Web`)
        #[arg(long = "libraries-only")]
        libraries_only: bool,
        /// Incluir subsitios de forma recursiva
        #[arg(long = "include-subsites")]
        include_subsites: bool,
        /// Subopción a gestionar: `all` (por defecto), `sharepoint` (solo sync SharePoint) o `onedrive` (solo accesos directos OneDrive)
        #[arg(long = "target", value_enum, default_value_t = SyncTarget::All)]
        target: SyncTarget,
        /// [Compatibilidad] No modificar el JSON View Formatting (`commandBarProps`) de las vistas de biblioteca (equivale a `--target sharepoint`)
        #[arg(long = "skip-view-formatting", conflicts_with = "target")]
        skip_view_formatting: bool,
        #[arg(long = "note")]
        note: Option<String>,
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
    /// Reactiva la sincronización offline de SharePoint y/o los accesos directos a OneDrive
    Enable {
        #[arg(long = "site")]
        site: String,
        /// Nombre(s) de biblioteca separados por coma (si se omite, aplica al sitio y todas sus bibliotecas)
        #[arg(long = "library", conflicts_with_all = ["site_only", "libraries_only", "include_subsites"])]
        library: Option<String>,
        /// Aplicar únicamente a nivel de sitio (`SP.Web`), sin modificar bibliotecas individuales
        #[arg(long = "site-only", conflicts_with = "libraries_only")]
        site_only: bool,
        /// Aplicar únicamente a las bibliotecas de documentos, sin modificar el sitio (`SP.Web`)
        #[arg(long = "libraries-only")]
        libraries_only: bool,
        /// Incluir subsitios de forma recursiva
        #[arg(long = "include-subsites")]
        include_subsites: bool,
        /// Subopción a gestionar: `all` (por defecto), `sharepoint` (solo sync SharePoint) o `onedrive` (solo accesos directos OneDrive)
        #[arg(long = "target", value_enum, default_value_t = SyncTarget::All)]
        target: SyncTarget,
        /// [Compatibilidad] No restaurar/quitar el JSON View Formatting (`commandBarProps`) de las vistas de biblioteca (equivale a `--target sharepoint`)
        #[arg(long = "skip-view-formatting", conflicts_with = "target")]
        skip_view_formatting: bool,
        #[arg(long = "note")]
        note: Option<String>,
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum SharepointSyncCommand {
    /// Bloquea la sincronización offline de SharePoint (`ExcludeFromOfflineClient = true`)
    Disable {
        #[arg(long = "site")]
        site: String,
        #[arg(long = "library", conflicts_with_all = ["site_only", "libraries_only", "include_subsites"])]
        library: Option<String>,
        #[arg(long = "site-only", conflicts_with = "libraries_only")]
        site_only: bool,
        #[arg(long = "libraries-only")]
        libraries_only: bool,
        #[arg(long = "include-subsites")]
        include_subsites: bool,
        #[arg(long = "note")]
        note: Option<String>,
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
    /// Permite la sincronización offline de SharePoint (`ExcludeFromOfflineClient = false`)
    Enable {
        #[arg(long = "site")]
        site: String,
        #[arg(long = "library", conflicts_with_all = ["site_only", "libraries_only", "include_subsites"])]
        library: Option<String>,
        #[arg(long = "site-only", conflicts_with = "libraries_only")]
        site_only: bool,
        #[arg(long = "libraries-only")]
        libraries_only: bool,
        #[arg(long = "include-subsites")]
        include_subsites: bool,
        #[arg(long = "note")]
        note: Option<String>,
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum OneDriveShortcutsCommand {
    /// Oculta el botón "Agregar acceso directo a OneDrive" en las vistas de biblioteca
    Disable {
        #[arg(long = "site")]
        site: String,
        #[arg(long = "library", conflicts_with = "include_subsites")]
        library: Option<String>,
        #[arg(long = "include-subsites")]
        include_subsites: bool,
        #[arg(long = "note")]
        note: Option<String>,
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
    /// Restaura el botón "Agregar acceso directo a OneDrive" en las vistas de biblioteca
    Enable {
        #[arg(long = "site")]
        site: String,
        #[arg(long = "library", conflicts_with = "include_subsites")]
        library: Option<String>,
        #[arg(long = "include-subsites")]
        include_subsites: bool,
        #[arg(long = "note")]
        note: Option<String>,
        #[arg(long = "dry-run")]
        dry_run: bool,
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
                email,
                tenant,
                client_id,
            } => {
                let mut target = email.clone().or_else(|| tenant.clone());
                if target.is_none()
                    && auth.detect_default_login_hint().is_none()
                    && !cli.non_interactive
                {
                    use std::io::Write;
                    eprint!("Correo corporativo M365 (ej. usuario@dominio.com): ");
                    let _ = std::io::stderr().flush();
                    let mut line = String::new();
                    std::io::stdin().read_line(&mut line)?;
                    let trimmed = line.trim().to_string();
                    if !trimmed.is_empty() {
                        target = Some(trimmed);
                    }
                }
                let sess = if *device_code {
                    let (prompt, t, c) = auth
                        .start_device_code(target.as_deref(), client_id.as_deref())
                        .await?;
                    eprintln!("{}", prompt.message);
                    auth.poll_device_code(&prompt, &t, &c).await?
                } else {
                    eprintln!("Abriendo el navegador para iniciar sesión en Microsoft 365...");
                    auth.login_interactive_browser(target.as_deref(), client_id.as_deref())
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
                    println!(
                        "{:<28} | {:<8} | {:<10} | {:<10} | RUTA RELATIVA",
                        "TÍTULO", "ITEMS", "DEFAULT", "SYNC"
                    );
                    println!("{}", "-".repeat(103));
                    for l in &libs {
                        println!(
                            "{:<28} | {:<8} | {:<10} | {:<10} | {}",
                            truncate(&l.title, 28),
                            l.item_count,
                            if l.is_default_documents { "Sí" } else { "No" },
                            if l.exclude_from_offline_client {
                                "Bloqueada"
                            } else {
                                "Activa"
                            },
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
            mode,
            confirm_destructive,
            note,
            dry_run,
        } => {
            if *mode == CloneModeArg::Destructive {
                if !*dry_run && !*confirm_destructive {
                    anyhow::bail!(
                        "⚠ MODO DESTRUCTIVO: esta operación ELIMINARÁ bibliotecas, carpetas, archivos, grupos y usuarios existentes en '{}' antes de clonar (no reversible por Rollback). Revisa primero el plan con `--dry-run` y añade `--confirm-destructive` para ejecutarlo.",
                        target_site
                    );
                }
                eprintln!(
                    "⚠ ADVERTENCIA [MODO DESTRUCTIVO]: Se eliminarán bibliotecas, carpetas, archivos, grupos y usuarios existentes en '{}' antes de clonar.",
                    target_site
                );
            }

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
                mode: match mode {
                    CloneModeArg::Additive => CloneMode::Additive,
                    CloneModeArg::Destructive => CloneMode::Destructive,
                },
                current_actor_upn: Some(actor_upn.clone()),
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
        Commands::Sync { action } => match action {
            SyncCommand::Status {
                site,
                include_subsites,
            } => {
                let status = spo.get_site_sync_status(site, *include_subsites).await?;
                print_output(&status, format, || {
                    print_site_sync_status_table(&status, 0);
                })?;
            }
            SyncCommand::Sharepoint { action } => match action {
                SharepointSyncCommand::Disable {
                    site,
                    library,
                    site_only,
                    libraries_only,
                    include_subsites,
                    note,
                    dry_run,
                } => {
                    handle_sync_toggle(
                        &spo,
                        &executor,
                        &actor_upn,
                        site,
                        library,
                        *site_only,
                        *libraries_only,
                        *include_subsites,
                        SyncTarget::Sharepoint,
                        note,
                        *dry_run,
                        true,
                        format,
                    )
                    .await?;
                }
                SharepointSyncCommand::Enable {
                    site,
                    library,
                    site_only,
                    libraries_only,
                    include_subsites,
                    note,
                    dry_run,
                } => {
                    handle_sync_toggle(
                        &spo,
                        &executor,
                        &actor_upn,
                        site,
                        library,
                        *site_only,
                        *libraries_only,
                        *include_subsites,
                        SyncTarget::Sharepoint,
                        note,
                        *dry_run,
                        false,
                        format,
                    )
                    .await?;
                }
            },
            SyncCommand::Onedrive { action } => match action {
                OneDriveShortcutsCommand::Disable {
                    site,
                    library,
                    include_subsites,
                    note,
                    dry_run,
                } => {
                    handle_sync_toggle(
                        &spo,
                        &executor,
                        &actor_upn,
                        site,
                        library,
                        false,
                        true,
                        *include_subsites,
                        SyncTarget::Onedrive,
                        note,
                        *dry_run,
                        true,
                        format,
                    )
                    .await?;
                }
                OneDriveShortcutsCommand::Enable {
                    site,
                    library,
                    include_subsites,
                    note,
                    dry_run,
                } => {
                    handle_sync_toggle(
                        &spo,
                        &executor,
                        &actor_upn,
                        site,
                        library,
                        false,
                        true,
                        *include_subsites,
                        SyncTarget::Onedrive,
                        note,
                        *dry_run,
                        false,
                        format,
                    )
                    .await?;
                }
            },
            SyncCommand::Disable {
                site,
                library,
                site_only,
                libraries_only,
                include_subsites,
                target,
                skip_view_formatting,
                note,
                dry_run,
            } => {
                let effective_target = if *skip_view_formatting {
                    SyncTarget::Sharepoint
                } else {
                    *target
                };
                handle_sync_toggle(
                    &spo,
                    &executor,
                    &actor_upn,
                    site,
                    library,
                    *site_only,
                    *libraries_only,
                    *include_subsites,
                    effective_target,
                    note,
                    *dry_run,
                    true,
                    format,
                )
                .await?;
            }
            SyncCommand::Enable {
                site,
                library,
                site_only,
                libraries_only,
                include_subsites,
                target,
                skip_view_formatting,
                note,
                dry_run,
            } => {
                let effective_target = if *skip_view_formatting {
                    SyncTarget::Sharepoint
                } else {
                    *target
                };
                handle_sync_toggle(
                    &spo,
                    &executor,
                    &actor_upn,
                    site,
                    library,
                    *site_only,
                    *libraries_only,
                    *include_subsites,
                    effective_target,
                    note,
                    *dry_run,
                    false,
                    format,
                )
                .await?;
            }
        },
    }

    Ok(())
}

fn enqueue_sync_from_status(
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
async fn handle_sync_toggle(
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

fn print_site_sync_status_table(status: &SiteSyncStatus, depth: usize) {
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

    #[test]
    fn test_sync_cli_parsing() {
        let status_cli = Cli::try_parse_from([
            "spoiberia",
            "sync",
            "status",
            "--site",
            "https://contoso.sharepoint.com/sites/Test",
            "--include-subsites",
            "--json",
        ])
        .unwrap();
        assert!(matches!(
            status_cli.command,
            Some(Commands::Sync {
                action: SyncCommand::Status {
                    include_subsites: true,
                    ..
                }
            })
        ));

        let disable_cli = Cli::try_parse_from([
            "spoiberia",
            "sync",
            "disable",
            "--site",
            "https://contoso.sharepoint.com/sites/Test",
            "--dry-run",
        ])
        .unwrap();
        assert!(matches!(
            disable_cli.command,
            Some(Commands::Sync {
                action: SyncCommand::Disable { dry_run: true, .. }
            })
        ));

        let disable_lib_cli = Cli::try_parse_from([
            "spoiberia",
            "sync",
            "disable",
            "--site",
            "https://contoso.sharepoint.com/sites/Test",
            "--library",
            "Documentos",
            "--skip-view-formatting",
        ])
        .unwrap();
        match disable_lib_cli.command {
            Some(Commands::Sync {
                action:
                    SyncCommand::Disable {
                        library,
                        skip_view_formatting,
                        ..
                    },
            }) => {
                assert_eq!(library.as_deref(), Some("Documentos"));
                assert!(skip_view_formatting);
            }
            other => panic!("unexpected parsed command: {:?}", other),
        }

        let enable_cli = Cli::try_parse_from([
            "spoiberia",
            "sync",
            "enable",
            "--site",
            "https://contoso.sharepoint.com/sites/Test",
            "--include-subsites",
        ])
        .unwrap();
        assert!(matches!(
            enable_cli.command,
            Some(Commands::Sync {
                action: SyncCommand::Enable {
                    include_subsites: true,
                    ..
                }
            })
        ));

        assert!(Cli::try_parse_from([
            "spoiberia",
            "sync",
            "disable",
            "--site",
            "https://contoso.sharepoint.com/sites/Test",
            "--library",
            "Documentos",
            "--site-only",
        ])
        .is_err());

        assert!(Cli::try_parse_from([
            "spoiberia",
            "sync",
            "enable",
            "--site",
            "https://contoso.sharepoint.com/sites/Test",
            "--site-only",
            "--libraries-only",
        ])
        .is_err());

        // Suboptions: `sync sharepoint ...`, `sync onedrive ...`, and `--target`
        let sp_disable = Cli::try_parse_from([
            "spoiberia",
            "sync",
            "sharepoint",
            "disable",
            "--site",
            "https://contoso.sharepoint.com/sites/Test",
            "--include-subsites",
        ])
        .unwrap();
        assert!(matches!(
            sp_disable.command,
            Some(Commands::Sync {
                action: SyncCommand::Sharepoint {
                    action: SharepointSyncCommand::Disable {
                        include_subsites: true,
                        ..
                    }
                }
            })
        ));

        let od_enable = Cli::try_parse_from([
            "spoiberia",
            "sync",
            "onedrive",
            "enable",
            "--site",
            "https://contoso.sharepoint.com/sites/Test",
            "--library",
            "Documentos",
        ])
        .unwrap();
        assert!(matches!(
            od_enable.command,
            Some(Commands::Sync {
                action: SyncCommand::Onedrive {
                    action: OneDriveShortcutsCommand::Enable { .. }
                }
            })
        ));

        let target_od = Cli::try_parse_from([
            "spoiberia",
            "sync",
            "disable",
            "--site",
            "https://contoso.sharepoint.com/sites/Test",
            "--target",
            "onedrive",
        ])
        .unwrap();
        assert!(matches!(
            target_od.command,
            Some(Commands::Sync {
                action: SyncCommand::Disable {
                    target: SyncTarget::Onedrive,
                    ..
                }
            })
        ));

        // `--skip-view-formatting` conflicts with `--target`
        assert!(Cli::try_parse_from([
            "spoiberia",
            "sync",
            "disable",
            "--site",
            "https://contoso.sharepoint.com/sites/Test",
            "--target",
            "all",
            "--skip-view-formatting",
        ])
        .is_err());
    }

    #[test]
    fn test_clone_mode_cli_parsing() {
        let additive = Cli::try_parse_from([
            "spoiberia",
            "clone",
            "--source-site",
            "https://contoso.sharepoint.com/sites/Src",
            "--target-site",
            "https://contoso.sharepoint.com/sites/Tgt",
        ])
        .unwrap();
        assert!(matches!(
            additive.command,
            Some(Commands::Clone {
                mode: CloneModeArg::Additive,
                confirm_destructive: false,
                ..
            })
        ));

        let destructive = Cli::try_parse_from([
            "spoiberia",
            "clone",
            "--source-site",
            "https://contoso.sharepoint.com/sites/Src",
            "--target-site",
            "https://contoso.sharepoint.com/sites/Tgt",
            "--mode",
            "destructive",
            "--confirm-destructive",
        ])
        .unwrap();
        assert!(matches!(
            destructive.command,
            Some(Commands::Clone {
                mode: CloneModeArg::Destructive,
                confirm_destructive: true,
                ..
            })
        ));
    }

    #[test]
    fn test_lang_flag_parsing() {
        let cli_en = Cli::try_parse_from(["spoiberia", "--lang", "en"]).unwrap();
        assert_eq!(cli_en.lang, crate::i18n::LangArg::En);

        let cli_es = Cli::try_parse_from(["spoiberia", "--lang", "es", "tui"]).unwrap();
        assert_eq!(cli_es.lang, crate::i18n::LangArg::Es);

        let cli_auto = Cli::try_parse_from(["spoiberia"]).unwrap();
        assert_eq!(cli_auto.lang, crate::i18n::LangArg::Auto);
    }

    #[test]
    fn test_help_output_is_colorized() {
        let mut cmd = Cli::command();
        cmd.build();

        let root_help = cmd.render_long_help().ansi().to_string();
        assert!(root_help.contains("\x1b["), "root help should contain ANSI escapes");
        assert!(
            root_help.contains("\x1b[33m")
                && root_help.contains("\x1b[36m")
                && root_help.contains("\x1b[32m"),
            "root help should contain yellow header, cyan literal, and green placeholder ANSI codes"
        );

        let sync_help = cmd
            .find_subcommand_mut("sync")
            .expect("sync subcommand exists")
            .render_long_help()
            .ansi()
            .to_string();
        assert!(
            sync_help.contains("\x1b["),
            "subcommand help should inherit ANSI escapes"
        );
        assert!(
            sync_help.contains("\x1b[33m") && sync_help.contains("\x1b[36m"),
            "subcommand help should inherit yellow header and cyan literal ANSI codes"
        );
    }
}
