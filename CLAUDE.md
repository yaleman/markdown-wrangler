# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

- Web interface to manage websites stored as folder structures of markdown files
- Designed to be compatible with Hugo and Zola static site generators
- Written in Rust using Cargo 2024 edition
- Uses justfile for task automation

## Development Commands

### Build and Run

```bash
cargo build          # Build the project
cargo run            # Run the application
cargo check          # Fast compile check
```

### Code Quality

```bash
cargo clippy          # Linting
cargo fmt             # Code formatting
cargo test            # Run tests
```

### Using Just

```bash
just                  # Show available tasks
just --list           # List all tasks
just lint             # Run cargo clippy --all-targets
just test             # Run cargo test
just check            # Run lint and test
```

## Architecture Notes

- Project is in early development stage with minimal implementation
- Single main.rs entry point currently contains placeholder code
- Dependencies include tokio (async runtime), clap (CLI parsing), axum (web framework), askama (templating), and serde (serialization)
- Intended to support both Hugo and Zola static site generator formats

## Development Guidelines

- Tasks aren't done until "just check" passes
- When your work is done on a request, you must commit it to git