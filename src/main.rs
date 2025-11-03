// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use markdown_wrangler::cli::Cli;
use markdown_wrangler::logging::{init_tracing, log_startup};
use markdown_wrangler::web::start_server;
use tokio::signal::unix::{SignalKind, signal};
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Validate CLI arguments
    if let Err(err) = cli.validate() {
        eprintln!("Error: {err}");
        std::process::exit(1);
    }

    let mut hangup_waiter = signal(SignalKind::hangup())?;
    let tracing_provider = init_tracing(cli.enable_otel_logs, cli.debug)?;
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
