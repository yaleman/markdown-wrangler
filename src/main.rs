// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

mod cli;
mod log_wrangler;
mod web;

use cli::Cli;
use log_wrangler::{init_tracing, log_startup};
use tokio::signal::unix::{SignalKind, signal};
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

    let mut hangup_waiter = signal(SignalKind::hangup())?;
    let tracing_provider = init_tracing(cli.debug)?;
    log_startup(cli.debug);

    info!("Watching directory: {}", cli.target_dir.display());

    tokio::select! {
        err = start_server(cli.target_dir) => {
            if let Err(err) = err {
                eprintln!("Server error, shutting down. Error: {err}");
            }
        },
        _ = hangup_waiter.recv() => {
            info!("Received SIGHUP, shutting down.");
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Received Ctrl-C, shutting down.");
        }
    }
    if let Err(err) = tracing_provider.shutdown() {
        eprintln!("Error shutting down tracing provider: {err}");
    }

    Ok(())
}
