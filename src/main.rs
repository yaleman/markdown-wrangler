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
        eprintln!("Error: {}", err);
        std::process::exit(1);
    }

    init_tracing(cli.debug)?;
    log_startup(cli.debug);

    info!("Watching directory: {}", cli.target_dir.display());

    start_server().await?;

    Ok(())
}
