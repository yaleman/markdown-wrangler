# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with
code in this repository.

## Project Overview

Markdown Wrangler is a web-based content management interface for markdown files, designed for Hugo and Zola static site generators. It provides a live-preview editor, file browser, and secure file operations with CSRF protection.

**Tech Stack:**
- Rust (Cargo 2024 edition, requires nightly toolchain)
- Tokio async runtime
- Axum web framework
- JavaScript (vanilla) for client-side functionality

## Development Commands

### Essential Commands

```bash
just check            # Run all quality checks (lint + test) - REQUIRED before commits
cargo run             # Start dev server on http://localhost:5420
cargo run -- --debug  # Run with debug logging
cargo run -- /path/to/content  # Run with custom content directory
cargo test            # Run all tests
cargo test test_name  # Run a single test
```

### Code Quality

```bash
just clippy           # Rust linting (treats warnings as errors)
just test             # Run Rust test suite
just fmt              # Check Rust formatting
pnpm run lint         # Lint JavaScript files (eslint)
```

### Docker

```bash
just docker_build     # Build multi-platform Docker container
```

## Architecture

### Application Flow

1. **CLI parsing** (`src/cli.rs`) - Validates target directory path
2. **Tracing initialization** (`src/log_wrangler.rs`) - Sets up OpenTelemetry
3. **Web server** (`src/web.rs`) - Starts Axum on 127.0.0.1:5420

### Security Architecture

**CSRF Protection:**
- All state-changing operations (save, delete) require CSRF tokens
- Tokens are HMAC-SHA256 signed with a random secret generated at startup
- Tokens expire after 1 hour (3600 seconds)
- Token format: `{timestamp}:{nonce}:{signature}`
- See `generate_csrf_token()` and `validate_csrf_token()` in `src/web.rs`

**File Safety:**
- Path traversal prevention via `canonicalize()` checks
- Executable files blocked from preview/serving
- Iframe preview limited to safe file types (txt, html, css, js, json, xml, pdf, csv, config files)
- Static file lists in constants: `IMAGE_EXTENSIONS`, `EXECUTABLE_EXTENSIONS`, `IFRAME_SAFE_EXTENSIONS`

### Key Functions in `src/web.rs`

**Route Handlers:**
- `index()` - Directory browser with breadcrumb navigation
- `edit_file()` - Markdown editor (GET /edit?path=...)
- `save_file()` - Save with CSRF validation, skips write if content unchanged
- `delete_file()` - Delete with CSRF validation
- `preview_image()` - Image preview page
- `serve_image()` - Serve image files with proper MIME types
- `preview_file()` - Generic file preview for non-markdown/non-image files
- `serve_file()` - Serve safe file types in iframes
- `get_file_info()` - JSON API for file metadata
- `get_file_content()` - JSON API for file content (markdown only)

**Security Helpers:**
- `validate_file_path()` - Canonicalization and boundary checks
- `is_markdown_file()` - .md/.markdown extension check
- `is_image_file()` - Image extension validation
- `is_executable_file()` - Executable detection
- `is_safe_for_iframe()` - Whitelist check for iframe previews

**HTML Generation:**
- All HTML is generated server-side (no templates currently)
- Functions: `generate_editor_html()`, `generate_directory_html()`, `generate_image_preview_html()`, `generate_file_preview_html()`
- Uses `html_escape` crate to prevent XSS

### Static Assets

Located in `/static/` directory:
- `editor.js` - Markdown live preview rendering
- `editor-storage.js` - LocalStorage draft management
- `delete.js` - Delete confirmation dialogs
- `styles.css` - Application styles

## Testing Strategy

**Comprehensive CSRF test coverage:**
- Token generation uniqueness
- Token validation (valid, invalid, expired, malformed)
- Protected endpoints reject missing/invalid tokens
- Edit page contains CSRF tokens in forms

**Integration tests use:**
- `tempfile::TempDir` for isolated test directories
- `tower::ServiceExt::oneshot()` for request testing
- Test helper: `create_test_app()` returns `(Router, TempDir, csrf_secret)`

**Run single test:**
```bash
cargo test test_csrf_token_validation -- --nocapture
```

## Development Guidelines

- **Mandatory:** Run `just check` before commits (no warnings/errors allowed)
- **Mandatory:** Commit completed work to git
- Use cargo commands for Cargo.toml changes (not manual edits)
- Keep CLAUDE.md updated with architecture changes
- JavaScript must be linted with eslint
- No inline JavaScript/CSS - use `/static/` files

## File Type Handling

**Markdown files (.md, .markdown):**
- Route to editor with live preview
- Can be saved and deleted

**Image files (jpg, jpeg, png, gif, webp, svg, bmp, tiff, tif):**
- Route to image preview with dimensions/metadata
- Served with correct MIME types

**Safe files (txt, html, css, js, json, xml, pdf, csv, yml, yaml, toml, ini, conf, cfg):**
- Route to file preview with iframe (if safe)
- Sandboxed iframe with `allow-same-origin`

**Executable files (exe, bat, cmd, com, scr, msi, sh, ps1, vbs, app, dmg, pkg, deb, rpm):**
- Display in browser but not clickable
- Blocked from preview/serving

## Common Pitfalls

- Don't modify `Cargo.toml` manually - use `cargo add/remove`
- CSRF tokens must be URL-encoded in form submissions
- File paths must be validated through `validate_file_path()` before use
- Always use `canonicalize()` for path security checks
- JavaScript linting uses pnpm, not npm
- Server binds to localhost only (127.0.0.1) - not accessible externally
