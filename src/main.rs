pub mod api;
pub mod app;
pub mod auth;
pub mod cli;
pub mod cloner;
pub mod db;
pub mod i18n;
pub mod importer;
pub mod models;
pub mod staging;
pub mod tui_input;
pub mod ui;

use crate::api::graph::GraphClient;
use crate::api::spo::SpoClient;
use crate::app::App;
use crate::auth::AuthManager;
use crate::cli::{Cli, Commands};
use crate::db::Database;
use crate::i18n::Lang;
use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let lang = Lang::resolve(cli.lang);

    match &cli.command {
        None | Some(Commands::Tui) => run_tui(lang).await,
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

async fn run_tui(lang: Lang) -> Result<()> {
    let auth = AuthManager::new()?;
    let db = Database::open_default()?;
    let graph = GraphClient::new(auth.clone())?;
    let spo = SpoClient::new(auth.clone())?;

    let mut app = App::new(auth, db, graph, spo, lang);

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
                    tui_input::handle_key_event(app, key);
                }
            }
        }
    }
    Ok(())
}
