# Markdown Wrangler

A web interface for managing websites stored as folder structures of markdown files, designed to be compatible with Hugo and Zola static site generators.

## Features

- Web-based markdown editor with live preview
- File browser for navigating markdown files
- CSRF protection for secure form submissions
- Local storage for draft management
- Support for markdown features:
  - Headers (H1-H6)
  - Bold and italic text
  - Strikethrough with double-tilde (`~~text~~`)
  - Ordered and unordered lists
  - Proper paragraph structure
- Toast notifications for user feedback
- Containerized deployment ready

## Prerequisites

- [Rust](https://rustlang.org/) (nightly toolchain for Cargo 2024 edition)
- [Node.js](https://nodejs.org/) (version 20 or later)
- [just](https://github.com/casey/just) (command runner)
- [Docker](https://docker.com/) (for containerized deployment)

## Installation

1. Clone the repository:
```bash
git clone <repository-url>
cd markdown-wrangler
```

2. Install JavaScript dependencies:
```bash
pnpm install
```

3. Install Rust dependencies (automatically handled by cargo):
```bash
cargo build
```

## Development

### Running the Application

Start the development server:
```bash
cargo run
```

The application will be available at `http://localhost:5420`.

### Available Commands

Use `just` for common development tasks:

```bash
just --list              # Show all available commands
just lint                # Run Rust linting (clippy)
just lint-js             # Run JavaScript linting (eslint)  
just test                # Run Rust tests
just check               # Run all quality checks (lint + test)
just fmt                 # Format JavaScript files
just docker-build        # Build Docker container
```

### Code Quality

Before submitting changes, ensure all quality checks pass:
```bash
just check
```

This runs:
- Rust linting with clippy (treating warnings as errors)
- JavaScript linting with eslint
- All Rust tests

### Testing

The project includes comprehensive tests for:
- CSRF token generation and validation
- Web endpoint protection
- File operations with temporary directories

Run tests with:
```bash
just test
# or
cargo test
```

## Building for Production

### Native Binary

Build an optimized release binary:
```bash
cargo build --release
```

The binary will be available at `target/release/markdown-wrangler`.

### Docker Container

Build the Docker container:
```bash
just docker-build
# or
docker build -t ghcr.io/yaleman/markdown-wrangler:latest .
```

The Dockerfile uses a multi-stage build:
- Build stage: `rustlang/rust:nightly-slim` (for Cargo 2024 edition support)
- Runtime stage: `gcr.io/distroless/cc-debian12` (minimal, secure runtime)

## Running as a Container

### Using Docker

Run the container locally:
```bash
docker run -p 5420:5420 -v /path/to/your/markdown/files:/data ghcr.io/yaleman/markdown-wrangler:latest
```

### Using Docker Compose

Create a `docker-compose.yml`:
```yaml
version: '3.8'
services:
  markdown-wrangler:
    image: ghcr.io/yaleman/markdown-wrangler:latest
    ports:
      - "5420:5420"
    volumes:
      - ./content:/data
    restart: unless-stopped
```

Run with:
```bash
docker compose up -d
```

## Configuration

### Command Line Options

```bash
markdown-wrangler [OPTIONS]

Options:
  -d, --debug     Enable debug logging
  -h, --help      Print help
  -V, --version   Print version
```

### Environment Variables

The application supports OpenTelemetry tracing. Configure with standard OpenTelemetry environment variables:

- `OTEL_EXPORTER_OTLP_ENDPOINT` - OpenTelemetry collector endpoint
- `OTEL_SERVICE_NAME` - Service name for tracing (defaults to "markdown-wrangler")

## Architecture

### Project Structure

```
├── src/
│   ├── main.rs           # Application entry point
│   ├── cli.rs            # Command line argument parsing
│   ├── log_wrangler.rs   # Tracing and OpenTelemetry setup
│   └── web.rs            # Web server and HTTP routing
├── static/
│   ├── editor.js         # Markdown editor functionality
│   ├── editor-storage.js # Local storage and draft management
│   └── styles.css        # Application styles
├── .github/workflows/    # CI/CD pipelines
├── Dockerfile           # Multi-stage container build
├── justfile            # Development commands
└── README.md           # This file
```

### Key Technologies

- **Rust**: Core application language with Cargo 2024 edition
- **Tokio**: Async runtime with tracing support
- **Axum**: Web framework for HTTP server
- **Clap**: Command line argument parsing
- **Tracing**: Structured logging and OpenTelemetry integration
- **Askama**: HTML templating
- **HMAC-SHA256**: CSRF token security

## Security

The application includes several security features:

- **CSRF Protection**: All state-changing endpoints require valid CSRF tokens
- **Token Expiration**: CSRF tokens expire after 1 hour
- **Secure Headers**: Proper HTTP security headers
- **Input Validation**: Sanitized file path handling
- **Container Security**: Distroless runtime container

## CI/CD

The project includes comprehensive GitHub Actions workflows:

- **CI** (`.github/workflows/ci.yml`): Runs tests, linting, and builds on PRs
- **Docker** (`.github/workflows/docker.yml`): Builds and pushes multi-platform containers to GHCR
- **Security** (`.github/workflows/security.yml`): Vulnerability scanning with Trivy and cargo-audit

Containers are automatically built and pushed to `ghcr.io/yaleman/markdown-wrangler` on pushes to the main branch.

## Static Site Generator Compatibility

Markdown Wrangler is designed to work with static site generators:

- **Hugo**: Compatible with Hugo's content structure and front matter
- **Zola**: Compatible with Zola's page structure and metadata

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Ensure `just check` passes
5. Submit a pull request

## License

This project is licensed under the Mozilla Public License 2.0 (MPL-2.0). See the [LICENSE](LICENSE) file for details.

The MPL-2.0 is a copyleft license that allows:
- Use in commercial and private projects
- Modification and distribution
- Patent protection
- Sublicensing under compatible licenses

Key requirements:
- Source code modifications must be shared under MPL-2.0
- Copyright and license notices must be preserved
- Changes to MPL-2.0 licensed files must be documented