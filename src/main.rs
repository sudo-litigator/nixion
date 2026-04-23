mod app;
mod nix;
mod ui;

use anyhow::Result;
use std::{env, path::PathBuf, process};

use crate::app::{App, AppOptions};
use crate::nix::NixClient;

fn main() -> Result<()> {
    match parse_cli_action()? {
        CliAction::Run(options) => {
            let mut terminal = ui::setup_terminal()?;
            let client = NixClient::default();
            let mut app = App::new_with_options(client, options);

            let run_result = app.init().and_then(|_| ui::run(&mut terminal, &mut app));
            let restore_result = ui::restore_terminal(&mut terminal);

            restore_result?;
            return run_result;
        }
        CliAction::PrintHelp => {
            print!("{}", help_text());
            return Ok(());
        }
        CliAction::PrintVersion => {
            println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
    }

}

enum CliAction {
    Run(AppOptions),
    PrintHelp,
    PrintVersion,
}

fn parse_cli_action() -> Result<CliAction> {
    let mut args = env::args();
    let _program = args.next();
    let mut options = AppOptions::default();

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "-h" | "--help" => return Ok(CliAction::PrintHelp),
            "-V" | "--version" => return Ok(CliAction::PrintVersion),
            "--flake" => {
                let Some(path) = args.next() else {
                    eprintln!("error: missing value for '--flake'\n\n{}", short_help_text());
                    process::exit(2);
                };
                options.flake_path = Some(PathBuf::from(path));
            }
            "--host" => {
                let Some(host) = args.next() else {
                    eprintln!("error: missing value for '--host'\n\n{}", short_help_text());
                    process::exit(2);
                };
                options.host = Some(host);
            }
            _ => {
                eprintln!(
                    "error: unexpected argument '{argument}'\n\n{}",
                    short_help_text()
                );
                process::exit(2);
            }
        }
    }

    Ok(CliAction::Run(options))
}

fn short_help_text() -> String {
    format!(
        "Usage: {name} [OPTIONS]\n\nTry '{name} --help' for more information.\n",
        name = env!("CARGO_PKG_NAME")
    )
}

fn help_text() -> String {
    format!(
        "{name} {version}\n{description}\n\nUsage:\n  {name} [OPTIONS]\n\nOptions:\n  --flake <path>   Open a specific flake instead of auto-detecting one\n  --host <name>    Preselect a nixosConfiguration host in the TUI\n  -h, --help       Print help\n  -V, --version    Print version\n\nNotes:\n  Start without arguments to open the terminal UI.\n  The TUI expects Nix with the modern CLI enabled.\n",
        name = env!("CARGO_PKG_NAME"),
        version = env!("CARGO_PKG_VERSION"),
        description = env!("CARGO_PKG_DESCRIPTION")
    )
}
