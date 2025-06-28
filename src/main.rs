use clap::Parser;
use opentelemetry::trace::TracerProvider;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(name = "markdown-wrangler")]
#[command(about = "A web interface to manage websites stored as markdown files")]
struct Cli {
    #[arg(long, help = "Enable debug logging")]
    debug: bool,
}

fn init_tracing(debug: bool) -> Result<(), Box<dyn std::error::Error>> {
    let filter = if debug {
        "debug"
    } else {
        "info"
    };

    if debug {
        let tracer = opentelemetry_sdk::trace::SdkTracerProvider::builder()
            .build()
            .tracer("markdown-wrangler");
        
        tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::new(filter))
            .with(tracing_subscriber::fmt::layer())
            .with(tracing_opentelemetry::layer().with_tracer(tracer))
            .init();
    } else {
        tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::new(filter))
            .with(tracing_subscriber::fmt::layer())
            .init();
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    
    init_tracing(cli.debug)?;
    
    info!("Starting markdown-wrangler");
    if cli.debug {
        info!("Debug mode enabled");
    }
    
    Ok(())
}
