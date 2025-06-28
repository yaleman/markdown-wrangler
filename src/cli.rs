use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "markdown-wrangler")]
#[command(about = "A web interface to manage websites stored as markdown files")]
pub struct Cli {
    #[arg(long, help = "Enable debug logging")]
    pub debug: bool,

    #[arg(
        help = "Target directory to watch for markdown files",
        default_value = ".",
        value_name = "DIR"
    )]
    pub target_dir: PathBuf,
}

impl Cli {
    pub fn parse() -> Self {
        Parser::parse()
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.target_dir.exists() {
            return Err(format!(
                "Target directory '{}' does not exist",
                self.target_dir.display()
            ));
        }

        if !self.target_dir.is_dir() {
            return Err(format!(
                "Target path '{}' is not a directory",
                self.target_dir.display()
            ));
        }

        Ok(())
    }
}