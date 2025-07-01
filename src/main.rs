// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

mod cli;
mod log_wrangler;
mod web;

use cli::Cli;
use log_wrangler::{init_tracing, log_startup};
use tracing::info;
use web::start_server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Validate CLI arguments
    if let Err(err) = cli.validate() {
        eprintln!("Error: {err}");
        std::process::exit(1);
    }

    init_tracing(cli.debug)?;
    log_startup(cli.debug);

    info!("Watching directory: {}", cli.target_dir.display());

    start_server(cli.target_dir).await?;

    Ok(())
}
