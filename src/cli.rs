// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

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

    #[arg(long, help = "Enable OpenTelemetry logging export")]
    pub enable_otel_logs: bool,
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

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::Parser;
    use std::{
        fs::File,
        path::{Path, PathBuf},
    };
    use tempfile::TempDir;

    #[test]
    fn test_parse_defaults() {
        let cli = Cli::parse_from(["markdown-wrangler"]);
        assert!(!cli.debug);
        assert!(!cli.enable_otel_logs);
        assert_eq!(cli.target_dir, PathBuf::from("."));
    }

    #[test]
    fn test_parse_flags_and_target_dir() {
        let cli = Cli::parse_from([
            "markdown-wrangler",
            "--debug",
            "--enable-otel-logs",
            "content",
        ]);
        assert!(cli.debug);
        assert!(cli.enable_otel_logs);
        assert_eq!(cli.target_dir, PathBuf::from("content"));
    }

    #[test]
    fn test_validate_success_for_existing_directory() {
        let temp_dir = TempDir::new().expect("failed to create temporary directory");
        let cli = Cli {
            debug: false,
            target_dir: temp_dir.path().to_path_buf(),
            enable_otel_logs: false,
        };
        assert!(cli.validate().is_ok());
    }

    #[test]
    fn test_validate_fails_for_missing_directory() {
        let temp_dir = TempDir::new().expect("failed to create temporary directory");
        let missing = temp_dir.path().join("does-not-exist");
        let cli = Cli {
            debug: false,
            target_dir: missing.clone(),
            enable_otel_logs: false,
        };

        let result = cli.validate();
        assert!(result.is_err());

        let err = result.expect_err("validation should return an error");
        assert!(err.contains("does not exist"));
        assert!(err.contains(&display_path(&missing)));
    }

    #[test]
    fn test_validate_fails_for_file_path() {
        let temp_dir = TempDir::new().expect("failed to create temporary directory");
        let file_path = temp_dir.path().join("file.md");
        File::create(&file_path).expect("failed to create temporary file");

        let cli = Cli {
            debug: false,
            target_dir: file_path.clone(),
            enable_otel_logs: false,
        };

        let result = cli.validate();
        assert!(result.is_err());

        let err = result.expect_err("validation should return an error");
        assert!(err.contains("is not a directory"));
        assert!(err.contains(&display_path(&file_path)));
    }

    fn display_path(path: &Path) -> String {
        path.display().to_string()
    }
}
