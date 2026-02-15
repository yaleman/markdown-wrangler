# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with
code in this repository.

## Project Overview

Markdown Wrangler is a web interface for browsing and editing markdown-centric
content trees (Hugo/Zola style), with file previews and guarded file
operations.

**Tech Stack:**

- Rust (Cargo edition 2024; project currently uses nightly in Docker/dev docs)
- Tokio async runtime
- Axum web framework
- Vanilla JavaScript for client behavior

## Development Commands

### Essential Commands

```bash
just check                 # Required before commits (clippy + tests + fmt check)
cargo run                  # Start dev server on http://127.0.0.1:5420
cargo run -- --debug       # Run with debug logging
cargo run -- /path/to/dir  # Run against a custom content directory
cargo test                 # Run all tests
cargo test test_name       # Run a single test
```

### Code Quality

```bash
just clippy                # Rust linting (warnings denied by policy)
just test                  # Rust tests
just fmt                   # Rust formatting check
just lint_js               # JavaScript linting through pnpm/eslint
pnpm run lint              # Direct eslint invocation for static/*.js
```

### Docker

```bash
just docker_build          # Build container image
```

## Architecture

### Application Flow

1. **CLI parsing/validation** (`src/cli.rs`)
2. **Tracing initialization** (`src/logging/mod.rs`)
3. **Web server startup** (`src/web/mod.rs`) on `127.0.0.1:5420`

### Routing (`src/web/mod.rs`)

- `GET /` - Directory browser
- `GET /edit?path=...` - Markdown editor
- `POST /save` - Save markdown content (CSRF-protected)
- `POST /delete` - Delete file (CSRF-protected)
- `GET /preview?path=...` - Image preview page
- `GET /image?path=...` - Image bytes endpoint
- `GET /file-preview?path=...` - Generic file preview page
- `GET /file?path=...` - Safe-file serving endpoint for iframe previews
- `GET /file-info?path=...` - JSON metadata
- `GET /file-content?path=...` - JSON content for markdown files
- `GET /static/*` - Static assets from `/static`

### Security Architecture

**CSRF Protection:**

- State-changing operations (`/save`, `/delete`) require CSRF tokens.
- Token format: `{timestamp}:{nonce}:{signature}`
- Tokens expire after 1 hour (3600 seconds).
- Secret is generated at startup from random bytes.
- Current signature algorithm is HMAC-SHA256 over `"{timestamp}:{nonce}"`,
  implemented in `generate_csrf_token()` /
  `validate_csrf_token()` in `src/web/mod.rs`.

**File Safety:**

- Path traversal controls use `canonicalize()` and base-directory prefix checks.
- `validate_file_path()` ensures resolved path is inside target dir and is a file.
- Executables are blocked from preview/serving routes.
- Iframe serving is extension allowlisted via `IFRAME_SAFE_EXTENSIONS`.

### Key Functions in `src/web/mod.rs`

**Route handlers:**

- `index()`, `edit_file()`, `save_file()`, `delete_file()`
- `preview_image()`, `serve_image()`
- `preview_file()`, `serve_file()`
- `get_file_info()`, `get_file_content()`

**Security helpers:**

- `validate_file_path()`
- `is_markdown_file()`
- `is_image_file()`
- `is_executable_file()`
- `is_safe_for_iframe()`

**HTML generation:**

- HTML is rendered with Askama templates in `templates/`.
- Template structs are defined in `src/web/mod.rs` (for directory, editor,
  image preview, file preview, and status pages).

### Static Assets (`/static`)

- `editor.js` - In-browser markdown preview rendering
- `editor-storage.js` - Local draft autosave and disk-conflict checks
- `delete.js` - Delete confirmation helper
- `styles.css` - Styling

## Testing Strategy

- Unit + integration tests are colocated in `src/web/mod.rs`.
- CSRF coverage includes generation, validity, malformed/expired token handling.
- Endpoint tests verify protected route behavior and editor token injection.
- Integration tests use:
  - `tempfile::TempDir`
  - `tower::ServiceExt::oneshot()`
  - helper `create_test_app()`

## Development Guidelines

- **Mandatory:** Run `just check` before commits.
- **Mandatory:** Commit completed work to git.
- Use cargo commands for dependency changes (do not hand-edit `Cargo.toml`).
- Keep `AGENTS.md` updated when architecture/behavior changes.
- JavaScript must pass eslint checks.
- Prefer static assets over inline JS/CSS.
- In production code, do not use `unwrap()` or `expect()`.
- In tests, `expect()` is allowed when the message adds actionable context.

## Known Gaps (as of current implementation)

- End-to-end test coverage for deleting a file from preview-page context is
  still missing.
- Add/adjust frontend tests or lint rules to prevent reintroducing inline
  JS/CSS policy violations.

## File Type Handling

**Markdown (`.md`, `.markdown`):**

- Open in editor with live preview, save, and delete flows.

**Images (`jpg`, `jpeg`, `png`, `gif`, `webp`, `svg`, `bmp`, `tiff`, `tif`):**

- Open in image preview and served with image MIME types.

**Safe iframe files (`txt`, `html`, `htm`, `css`, `js`, `json`, `xml`, `pdf`,
`csv`, `log`, `yml`, `yaml`, `toml`, `ini`, `conf`, `cfg`):**

- Open in generic file preview; `/file` serves bytes with type + safety headers.

**Executable extensions (`exe`, `bat`, `cmd`, `com`, `scr`, `msi`, `sh`, `ps1`,
`vbs`, `app`, `dmg`, `pkg`, `deb`, `rpm`):**

- Shown in listing but intentionally not linked/served for preview.

## Common Pitfalls

- Do not modify `Cargo.toml` manually for dependencies.
- URL-encode CSRF tokens in form submissions.
- Always pass file paths through `validate_file_path()` before file operations.
- Keep path checks canonicalized and bounded to target dir.
- Server binds to localhost only (`127.0.0.1:5420`).
