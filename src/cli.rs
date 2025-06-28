use clap::Parser;

#[derive(Parser)]
#[command(name = "markdown-wrangler")]
#[command(about = "A web interface to manage websites stored as markdown files")]
pub struct Cli {
    #[arg(long, help = "Enable debug logging")]
    pub debug: bool,
}

impl Cli {
    pub fn parse() -> Self {
        Parser::parse()
    }
}