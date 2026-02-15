// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

#![deny(warnings)]
#![deny(deprecated)]
#![recursion_limit = "512"]
#![deny(unused_extern_crates)]
// Enable some groups of clippy lints.
#![deny(clippy::suspicious)]
#![deny(clippy::perf)]
// Specific lints to enforce.
#![deny(clippy::todo)]
#![deny(clippy::unimplemented)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::await_holding_lock)]
#![deny(clippy::needless_pass_by_value)]
#![deny(clippy::trivially_copy_pass_by_ref)]
#![deny(clippy::disallowed_types)]
#![deny(clippy::manual_let_else)]
#![deny(clippy::indexing_slicing)]
#![deny(clippy::unreachable)]

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

    info!(
        "Watching directory: {} (max upload size: {} bytes)",
        cli.target_dir.display(),
        cli.max_upload_size_bytes
    );

    tokio::select! {
        err = start_server(cli.target_dir, cli.max_upload_size_bytes) => {
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
