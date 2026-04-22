mod app;
mod nix;
mod ui;

use anyhow::Result;

use crate::app::App;
use crate::nix::NixClient;

fn main() -> Result<()> {
    let mut terminal = ui::setup_terminal()?;
    let client = NixClient::default();
    let mut app = App::new(client);

    let run_result = app.init().and_then(|_| ui::run(&mut terminal, &mut app));
    let restore_result = ui::restore_terminal(&mut terminal);

    restore_result?;
    run_result
}
