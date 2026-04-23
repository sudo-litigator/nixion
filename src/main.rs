mod app;
mod nix;
mod ui;

use anyhow::Result;
use std::{env, process};

use crate::app::App;
use crate::nix::NixClient;

fn main() -> Result<()> {
    match parse_cli_action()? {
        CliAction::Run => {}
        CliAction::PrintHelp => {
            print!("{}", help_text());
            return Ok(());
        }
        CliAction::PrintVersion => {
            println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
    }

    let mut terminal = ui::setup_terminal()?;
    let client = NixClient::default();
    let mut app = App::new(client);

    let run_result = app.init().and_then(|_| ui::run(&mut terminal, &mut app));
    let restore_result = ui::restore_terminal(&mut terminal);

    restore_result?;
    run_result
}

enum CliAction {
    Run,
    PrintHelp,
    PrintVersion,
}

fn parse_cli_action() -> Result<CliAction> {
    let mut args = env::args();
    let _program = args.next();

    match args.next().as_deref() {
        None => Ok(CliAction::Run),
        Some("-h") | Some("--help") => Ok(CliAction::PrintHelp),
        Some("-V") | Some("--version") => Ok(CliAction::PrintVersion),
        Some(argument) => {
            eprintln!(
                "error: unexpected argument '{argument}'\n\n{}",
                short_help_text()
            );
            process::exit(2);
        }
    }
}

fn short_help_text() -> String {
    format!(
        "Usage: {name} [OPTIONS]\n\nTry '{name} --help' for more information.\n",
        name = env!("CARGO_PKG_NAME")
    )
}

fn help_text() -> String {
    format!(
        "{name} {version}\n{description}\n\nUsage:\n  {name} [OPTIONS]\n\nOptions:\n  -h, --help       Print help\n  -V, --version    Print version\n\nNotes:\n  Start without arguments to open the terminal UI.\n  The TUI expects Nix with the modern CLI enabled.\n",
        name = env!("CARGO_PKG_NAME"),
        version = env!("CARGO_PKG_VERSION"),
        description = env!("CARGO_PKG_DESCRIPTION")
    )
}
