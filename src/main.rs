mod cli;
mod log_wrangler;
mod web;

use cli::Cli;
use log_wrangler::{init_tracing, log_startup};
use web::start_server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    init_tracing(cli.debug)?;
    log_startup(cli.debug);

    start_server().await?;

    Ok(())
}
