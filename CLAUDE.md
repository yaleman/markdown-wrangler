# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with
code in this repository.

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

### Project Structure

- `src/main.rs` - Application entry point with async runtime and module
  orchestration
- `src/cli.rs` - Command line argument parsing using clap
- `src/log_wrangler.rs` - Tracing and OpenTelemetry initialization
- `src/web.rs` - Axum web server with HTTP routing and middleware

### Key Dependencies

- **tokio** - Async runtime with full features and tracing support
- **axum** - Web framework for HTTP server functionality
- **clap** - CLI argument parsing with derive features
- **tracing ecosystem** - Structured logging and OpenTelemetry integration
  - `tracing` - Core tracing library
  - `tracing-subscriber` - Subscriber implementations with env-filter
  - `tracing-opentelemetry` - OpenTelemetry integration with metrics
  - `axum-tracing-opentelemetry` - Axum middleware for request tracing
- **askama** - HTML templating with serde_json support
- **serde** - Serialization framework with derive macros

### Web Server

- Listens on port 5420 (0.0.0.0:5420)
- Routes: `/` returns "Hello World", all other paths return 404 "not found"
- Includes OpenTelemetry tracing middleware for request monitoring
- Supports both normal and debug logging modes via --debug flag

### Static Site Generator Compatibility

- Intended to support both Hugo and Zola static site generator formats

## Development Guidelines

- Tasks aren't done until "just check" passes
- When your work is done on a request, you must commit it to git
- Seriously it's mandatory to git commit things at the end of your task

## Testing

- Add test coverage for sensible paths on any new code

## Quality Assurance

- Your task is not complete unless "just check" passes without warnings or errors

## Web Development Best Practices

- Don't use inline javascript or css, use static files and serve them from a /static/ directory

## JavaScript and Linting

- eslint is what I use for javascript linting
- prettier is what I use for javascript formatting