// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use askama::Template;
use axum::{
    Router,
    extract::{Form, Query, State},
    http::StatusCode,
    response::{Html, Json},
    routing::{get, post},
};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::net::TcpListener;
use tower_http::services::ServeDir;
use tracing::{info, warn};

#[derive(Clone)]
pub struct AppState {
    pub target_dir: PathBuf,
    pub csrf_secret: String,
}

#[derive(Debug)]
struct DirectoryEntry {
    name: String,
    is_directory: bool,
    path: String,
}

#[derive(Debug)]
struct Breadcrumb {
    name: String,
    url: String,
}

#[derive(Debug)]
struct DirectoryEntryView {
    icon: &'static str,
    class_name: &'static str,
    name: String,
    url: String,
    has_url: bool,
    executable: bool,
}

#[derive(Template)]
#[template(path = "directory.html")]
struct DirectoryTemplate {
    at_root: bool,
    breadcrumbs: Vec<Breadcrumb>,
    has_parent: bool,
    parent_url: String,
    entries: Vec<DirectoryEntryView>,
}

#[derive(Template)]
#[template(path = "editor.html")]
struct EditorTemplate {
    file_path: String,
    content: String,
    csrf_token: String,
}

#[derive(Template)]
#[template(path = "image_preview.html")]
struct ImagePreviewTemplate {
    file_path: String,
    encoded_path: String,
    file_size: String,
    parent_path: String,
    csrf_token: String,
}

#[derive(Template)]
#[template(path = "file_preview.html")]
struct FilePreviewTemplate {
    file_path: String,
    encoded_path: String,
    file_size: String,
    file_type: String,
    parent_path: String,
    csrf_token: String,
    can_iframe: bool,
}

#[derive(Template)]
#[template(path = "status_page.html")]
struct StatusPageTemplate {
    title: String,
    heading: String,
    heading_class: String,
    file_path: String,
    detail_text: String,
    show_edit_button: bool,
    edit_url: String,
    back_url: String,
}

#[derive(Deserialize)]
struct EditForm {
    path: String,
    content: String,
    csrf_token: String,
}

#[derive(Deserialize)]
struct DeleteForm {
    path: String,
    csrf_token: String,
}

#[derive(Serialize)]
struct FileInfo {
    modified_time: String,
    size: u64,
}

#[derive(Serialize)]
struct FileContent {
    content: String,
    modified_time: String,
}

pub(crate) fn generate_csrf_token(secret: &str) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let nonce: u64 = rand::rng().random();

    let payload = format!("{timestamp}:{nonce}");
    let mut hasher = Sha256::new();
    hasher.update(payload.as_bytes());
    hasher.update(secret.as_bytes());
    let signature = hex::encode(hasher.finalize());

    format!("{payload}:{signature}")
}

pub(crate) fn validate_csrf_token(token: &str, secret: &str) -> bool {
    let parts: Vec<&str> = token.split(':').collect();
    if parts.len() != 3 {
        return false;
    }

    let timestamp_str = parts[0];
    let nonce = parts[1];
    let provided_signature = parts[2];

    // Check if token is not too old (1 hour)
    if let Ok(timestamp) = timestamp_str.parse::<u64>() {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        if current_time - timestamp > 3600 {
            return false;
        }
    } else {
        return false;
    }

    let payload = format!("{timestamp_str}:{nonce}");
    let mut hasher = Sha256::new();
    hasher.update(payload.as_bytes());
    hasher.update(secret.as_bytes());
    let expected_signature = hex::encode(hasher.finalize());

    expected_signature == provided_signature
}

fn list_directory(base_dir: &Path, relative_path: &str) -> Result<Vec<DirectoryEntry>, String> {
    let full_path = if relative_path.is_empty() {
        base_dir.to_path_buf()
    } else {
        base_dir.join(relative_path)
    };

    // Security check: ensure the path is within the base directory
    let canonical_base = base_dir
        .canonicalize()
        .map_err(|e| format!("Base directory error: {e}"))?;
    let canonical_full = full_path
        .canonicalize()
        .map_err(|e| format!("Path error: {e}"))?;

    if !canonical_full.starts_with(&canonical_base) {
        return Err("Path outside base directory".to_string());
    }

    let entries = fs::read_dir(&full_path).map_err(|e| format!("Failed to read directory: {e}"))?;

    let mut directory_entries = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read entry: {e}"))?;
        let file_name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden files
        if file_name.starts_with('.') {
            continue;
        }

        let is_directory = entry
            .file_type()
            .map_err(|e| format!("Failed to get file type: {e}"))?
            .is_dir();

        let entry_path = if relative_path.is_empty() {
            file_name.clone()
        } else {
            format!("{relative_path}/{file_name}")
        };

        directory_entries.push(DirectoryEntry {
            name: file_name,
            is_directory,
            path: entry_path,
        });
    }

    // Sort directories first, then files
    directory_entries.sort_by(|a, b| match (a.is_directory, b.is_directory) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    Ok(directory_entries)
}

fn validate_file_path(base_dir: &Path, relative_path: &str) -> Result<PathBuf, String> {
    let full_path = base_dir.join(relative_path);

    // Security check: ensure the path is within the base directory
    let canonical_base = base_dir
        .canonicalize()
        .map_err(|e| format!("Base directory error: {e}"))?;
    let canonical_full = full_path
        .canonicalize()
        .map_err(|_| "Path does not exist".to_string())?;

    if !canonical_full.starts_with(&canonical_base) {
        return Err("Path outside base directory".to_string());
    }

    if !canonical_full.is_file() {
        return Err("Path is not a file".to_string());
    }

    Ok(canonical_full)
}

fn is_markdown_file(path: &str) -> bool {
    path.to_lowercase().ends_with(".md") || path.to_lowercase().ends_with(".markdown")
}

const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "svg", "bmp", "tiff", "tif",
];

fn is_image_file(path: &str) -> bool {
    let lower_path = path.to_lowercase();
    IMAGE_EXTENSIONS.contains(&lower_path.split('.').next_back().unwrap_or(""))
}

const EXECUTABLE_EXTENSIONS: &[&str] = &[
    "exe", "bat", "cmd", "com", "scr", "msi", "sh", "ps1", "vbs", "app", "dmg", "pkg", "deb", "rpm",
];

fn is_executable_file(path: &str) -> bool {
    let lower_path = path.to_lowercase();
    EXECUTABLE_EXTENSIONS.contains(&lower_path.split('.').next_back().unwrap_or(""))
}

const IFRAME_SAFE_EXTENSIONS: &[&str] = &[
    "txt", "html", "htm", "css", "js", "json", "xml", "pdf", "csv", "log", "yml", "yaml", "toml",
    "ini", "conf", "cfg",
];

fn is_safe_for_iframe(path: &str) -> bool {
    let lower_path = path.to_lowercase();
    // Allow text files, web files, and documents that browsers can display safely
    IFRAME_SAFE_EXTENSIONS.contains(&lower_path.split('.').next_back().unwrap_or(""))
}

fn read_file_content(file_path: &Path) -> Result<String, String> {
    fs::read_to_string(file_path).map_err(|e| format!("Failed to read file: {e}"))
}

fn write_file_content(file_path: &Path, content: &str) -> Result<(), String> {
    fs::write(file_path, content).map_err(|e| format!("Failed to write file: {e}"))
}

fn get_file_size(file_path: &Path) -> Result<u64, String> {
    fs::metadata(file_path)
        .map(|metadata| metadata.len())
        .map_err(|e| format!("Failed to get file metadata: {e}"))
}

fn format_file_size(size_bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = size_bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{size_bytes} {}", UNITS[unit_index])
    } else {
        format!("{size:.1} {}", UNITS[unit_index])
    }
}

fn get_parent_directory_path(file_path: &str) -> String {
    if let Some(parent) = std::path::Path::new(file_path).parent() {
        let parent_str = parent.to_string_lossy();
        if parent_str.is_empty() || parent_str == "." {
            "/".to_string()
        } else {
            format!("/?path={}", urlencoding::encode(&parent_str))
        }
    } else {
        "/".to_string()
    }
}

fn get_file_modification_time(file_path: &Path) -> Result<String, String> {
    fs::metadata(file_path)
        .and_then(|metadata| metadata.modified())
        .map(|time| {
            time.duration_since(SystemTime::UNIX_EPOCH)
                .map(|duration| duration.as_secs().to_string())
                .unwrap_or_else(|_| "0".to_string())
        })
        .map_err(|e| format!("Failed to get file modification time: {e}"))
}

fn render_template<T: Template>(template: &T) -> Result<String, String> {
    template
        .render()
        .map_err(|e| format!("Failed to render template: {e}"))
}

fn generate_editor_html(
    file_path: &str,
    content: &str,
    csrf_token: &str,
) -> Result<String, String> {
    let template = EditorTemplate {
        file_path: file_path.to_string(),
        content: content.to_string(),
        csrf_token: csrf_token.to_string(),
    };
    render_template(&template)
}

fn generate_image_preview_html(
    file_path: &str,
    file_size: &str,
    csrf_token: &str,
) -> Result<String, String> {
    let template = ImagePreviewTemplate {
        file_path: file_path.to_string(),
        encoded_path: urlencoding::encode(file_path).into_owned(),
        file_size: file_size.to_string(),
        parent_path: get_parent_directory_path(file_path),
        csrf_token: csrf_token.to_string(),
    };
    render_template(&template)
}

fn generate_file_preview_html(
    file_path: &str,
    file_size: &str,
    csrf_token: &str,
) -> Result<String, String> {
    let template = FilePreviewTemplate {
        file_path: file_path.to_string(),
        encoded_path: urlencoding::encode(file_path).into_owned(),
        file_size: file_size.to_string(),
        file_type: get_file_type_description(file_path).to_string(),
        parent_path: get_parent_directory_path(file_path),
        csrf_token: csrf_token.to_string(),
        can_iframe: is_safe_for_iframe(file_path),
    };
    render_template(&template)
}

fn get_file_type_description(file_path: &str) -> &'static str {
    let lower_path = file_path.to_lowercase();

    if lower_path.ends_with(".txt") {
        "Text file"
    } else if lower_path.ends_with(".html") || lower_path.ends_with(".htm") {
        "HTML document"
    } else if lower_path.ends_with(".css") {
        "CSS stylesheet"
    } else if lower_path.ends_with(".js") {
        "JavaScript file"
    } else if lower_path.ends_with(".json") {
        "JSON data"
    } else if lower_path.ends_with(".xml") {
        "XML document"
    } else if lower_path.ends_with(".pdf") {
        "PDF document"
    } else if lower_path.ends_with(".csv") {
        "CSV data"
    } else if lower_path.ends_with(".log") {
        "Log file"
    } else if lower_path.ends_with(".yml") || lower_path.ends_with(".yaml") {
        "YAML configuration"
    } else if lower_path.ends_with(".toml") {
        "TOML configuration"
    } else if lower_path.ends_with(".ini")
        || lower_path.ends_with(".conf")
        || lower_path.ends_with(".cfg")
    {
        "Configuration file"
    } else if is_executable_file(file_path) {
        "Executable file"
    } else {
        "Unknown file type"
    }
}

fn build_breadcrumbs(current_path: &str) -> Vec<Breadcrumb> {
    let mut breadcrumbs = Vec::new();
    let mut path_so_far = String::new();

    for part in current_path.split('/') {
        if !path_so_far.is_empty() {
            path_so_far.push('/');
        }
        path_so_far.push_str(part);
        breadcrumbs.push(Breadcrumb {
            name: part.to_string(),
            url: format!("/?path={}", urlencoding::encode(&path_so_far)),
        });
    }

    breadcrumbs
}

fn build_directory_entry_views(entries: &[DirectoryEntry]) -> Vec<DirectoryEntryView> {
    entries
        .iter()
        .map(|entry| {
            if entry.is_directory {
                DirectoryEntryView {
                    icon: "📁",
                    class_name: "directory",
                    name: entry.name.clone(),
                    url: format!("/?path={}", urlencoding::encode(&entry.path)),
                    has_url: true,
                    executable: false,
                }
            } else if is_markdown_file(&entry.name) {
                DirectoryEntryView {
                    icon: "📄",
                    class_name: "file",
                    name: entry.name.clone(),
                    url: format!("/edit?path={}", urlencoding::encode(&entry.path)),
                    has_url: true,
                    executable: false,
                }
            } else if is_image_file(&entry.name) {
                DirectoryEntryView {
                    icon: "🖼️",
                    class_name: "file",
                    name: entry.name.clone(),
                    url: format!("/preview?path={}", urlencoding::encode(&entry.path)),
                    has_url: true,
                    executable: false,
                }
            } else if is_executable_file(&entry.name) {
                DirectoryEntryView {
                    icon: "⚠️",
                    class_name: "file",
                    name: entry.name.clone(),
                    url: String::new(),
                    has_url: false,
                    executable: true,
                }
            } else {
                DirectoryEntryView {
                    icon: "📄",
                    class_name: "file",
                    name: entry.name.clone(),
                    url: format!("/file-preview?path={}", urlencoding::encode(&entry.path)),
                    has_url: true,
                    executable: false,
                }
            }
        })
        .collect()
}

fn generate_directory_html(
    entries: &[DirectoryEntry],
    current_path: &str,
) -> Result<String, String> {
    let parent_url = if let Some(pos) = current_path.rfind('/') {
        let parent_path = &current_path[..pos];
        if parent_path.is_empty() {
            "/".to_string()
        } else {
            format!("/?path={}", urlencoding::encode(parent_path))
        }
    } else {
        "/".to_string()
    };

    let template = DirectoryTemplate {
        at_root: current_path.is_empty(),
        breadcrumbs: build_breadcrumbs(current_path),
        has_parent: !current_path.is_empty(),
        parent_url,
        entries: build_directory_entry_views(entries),
    };
    render_template(&template)
}

struct StatusPageContext<'a> {
    title: &'a str,
    heading: &'a str,
    heading_class: &'a str,
    file_path: &'a str,
    detail_text: &'a str,
    show_edit_button: bool,
    edit_url: &'a str,
    back_url: &'a str,
}

fn generate_status_html(context: StatusPageContext<'_>) -> Result<String, String> {
    let template = StatusPageTemplate {
        title: context.title.to_string(),
        heading: context.heading.to_string(),
        heading_class: context.heading_class.to_string(),
        file_path: context.file_path.to_string(),
        detail_text: context.detail_text.to_string(),
        show_edit_button: context.show_edit_button,
        edit_url: context.edit_url.to_string(),
        back_url: context.back_url.to_string(),
    };
    render_template(&template)
}

async fn index(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<Html<String>, (StatusCode, String)> {
    let path = params.get("path").map(|s| s.as_str()).unwrap_or("");

    match list_directory(&state.target_dir, path) {
        Ok(entries) => match generate_directory_html(&entries, path) {
            Ok(html) => Ok(Html(html)),
            Err(err) => {
                warn!("Template render error: {}", err);
                Err((StatusCode::INTERNAL_SERVER_ERROR, err))
            }
        },
        Err(err) => {
            warn!("Directory listing error: {}", err);
            Err((StatusCode::BAD_REQUEST, format!("Error: {err}")))
        }
    }
}

async fn edit_file(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<Html<String>, (StatusCode, String)> {
    let file_path = params.get("path").ok_or((
        StatusCode::BAD_REQUEST,
        "Missing path parameter".to_string(),
    ))?;

    if !is_markdown_file(file_path) {
        return Err((
            StatusCode::BAD_REQUEST,
            "File is not a markdown file".to_string(),
        ));
    }

    match validate_file_path(&state.target_dir, file_path) {
        Ok(full_path) => match read_file_content(&full_path) {
            Ok(content) => {
                let csrf_token = generate_csrf_token(&state.csrf_secret);
                match generate_editor_html(file_path, &content, &csrf_token) {
                    Ok(html) => Ok(Html(html)),
                    Err(err) => {
                        warn!("Template render error: {}", err);
                        Err((StatusCode::INTERNAL_SERVER_ERROR, err))
                    }
                }
            }
            Err(err) => {
                warn!("File read error: {}", err);
                Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Error reading file: {err}"),
                ))
            }
        },
        Err(err) => {
            warn!("File validation error: {}", err);
            Err((StatusCode::BAD_REQUEST, format!("Error: {err}")))
        }
    }
}

async fn save_file(
    State(state): State<AppState>,
    Form(form): Form<EditForm>,
) -> Result<Html<String>, (StatusCode, String)> {
    // Validate CSRF token
    if !validate_csrf_token(&form.csrf_token, &state.csrf_secret) {
        return Err((StatusCode::FORBIDDEN, "Invalid CSRF token".to_string()));
    }

    if !is_markdown_file(&form.path) {
        return Err((
            StatusCode::BAD_REQUEST,
            "File is not a markdown file".to_string(),
        ));
    }

    match validate_file_path(&state.target_dir, &form.path) {
        Ok(full_path) => {
            // Read existing content to check if it has changed
            match read_file_content(&full_path) {
                Ok(existing_content) => {
                    if existing_content == form.content {
                        // Content hasn't changed, don't write to disk
                        info!("File content unchanged, skipping write: {}", form.path);
                        let parent_path = get_parent_directory_path(&form.path);
                        let edit_url = format!("/edit?path={}", urlencoding::encode(&form.path));
                        match generate_status_html(StatusPageContext {
                            title: "File Unchanged - Markdown Wrangler",
                            heading: "ℹ️ No Changes to Save",
                            heading_class: "success",
                            file_path: &form.path,
                            detail_text: "content is unchanged.",
                            show_edit_button: true,
                            edit_url: &edit_url,
                            back_url: &parent_path,
                        }) {
                            Ok(html) => Ok(Html(html)),
                            Err(err) => {
                                warn!("Template render error: {}", err);
                                Err((StatusCode::INTERNAL_SERVER_ERROR, err))
                            }
                        }
                    } else {
                        // Content has changed, write to disk
                        match write_file_content(&full_path, &form.content) {
                            Ok(()) => {
                                info!("File saved successfully: {}", form.path);
                                let parent_path = get_parent_directory_path(&form.path);
                                let edit_url =
                                    format!("/edit?path={}", urlencoding::encode(&form.path));
                                match generate_status_html(StatusPageContext {
                                    title: "File Saved - Markdown Wrangler",
                                    heading: "✅ File Saved Successfully!",
                                    heading_class: "success",
                                    file_path: &form.path,
                                    detail_text: "has been saved.",
                                    show_edit_button: true,
                                    edit_url: &edit_url,
                                    back_url: &parent_path,
                                }) {
                                    Ok(html) => Ok(Html(html)),
                                    Err(err) => {
                                        warn!("Template render error: {}", err);
                                        Err((StatusCode::INTERNAL_SERVER_ERROR, err))
                                    }
                                }
                            }
                            Err(err) => {
                                warn!("File save error: {}", err);
                                Err((
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    format!("Error saving file: {err}"),
                                ))
                            }
                        }
                    }
                }
                Err(err) => {
                    warn!("File read error during save comparison: {}", err);
                    Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Error reading file for comparison: {err}"),
                    ))
                }
            }
        }
        Err(err) => {
            warn!("File validation error during save: {}", err);
            Err((StatusCode::BAD_REQUEST, format!("Error: {err}")))
        }
    }
}

async fn preview_image(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<Html<String>, (StatusCode, String)> {
    let file_path = params.get("path").ok_or((
        StatusCode::BAD_REQUEST,
        "Missing path parameter".to_string(),
    ))?;

    if !is_image_file(file_path) {
        return Err((
            StatusCode::BAD_REQUEST,
            "File is not an image file".to_string(),
        ));
    }

    match validate_file_path(&state.target_dir, file_path) {
        Ok(full_path) => {
            let csrf_token = generate_csrf_token(&state.csrf_secret);
            match get_file_size(&full_path) {
                Ok(size_bytes) => {
                    let file_size = format_file_size(size_bytes);
                    match generate_image_preview_html(file_path, &file_size, &csrf_token) {
                        Ok(html) => Ok(Html(html)),
                        Err(err) => {
                            warn!("Template render error: {}", err);
                            Err((StatusCode::INTERNAL_SERVER_ERROR, err))
                        }
                    }
                }
                Err(err) => {
                    warn!("Failed to get file size: {}", err);
                    // Fall back to generating without size info
                    match generate_image_preview_html(file_path, "Unknown", &csrf_token) {
                        Ok(html) => Ok(Html(html)),
                        Err(render_err) => {
                            warn!("Template render error: {}", render_err);
                            Err((StatusCode::INTERNAL_SERVER_ERROR, render_err))
                        }
                    }
                }
            }
        }
        Err(err) => {
            warn!("File validation error: {}", err);
            Err((StatusCode::BAD_REQUEST, format!("Error: {err}")))
        }
    }
}

async fn serve_image(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    let file_path = params.get("path").ok_or((
        StatusCode::BAD_REQUEST,
        "Missing path parameter".to_string(),
    ))?;

    if !is_image_file(file_path) {
        return Err((
            StatusCode::BAD_REQUEST,
            "File is not an image file".to_string(),
        ));
    }

    match validate_file_path(&state.target_dir, file_path) {
        Ok(full_path) => {
            match fs::read(&full_path) {
                Ok(file_contents) => {
                    // Determine content type based on file extension
                    let content_type = match full_path.extension().and_then(|s| s.to_str()) {
                        Some("jpg") | Some("jpeg") => "image/jpeg",
                        Some("png") => "image/png",
                        Some("gif") => "image/gif",
                        Some("webp") => "image/webp",
                        Some("svg") => "image/svg+xml",
                        Some("bmp") => "image/bmp",
                        Some("tiff") | Some("tif") => "image/tiff",
                        _ => "application/octet-stream",
                    };

                    Ok(axum::response::Response::builder()
                        .header("Content-Type", content_type)
                        .body(axum::body::Body::from(file_contents))
                        .unwrap())
                }
                Err(err) => {
                    warn!("Image read error: {}", err);
                    Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Error reading image: {err}"),
                    ))
                }
            }
        }
        Err(err) => {
            warn!("Image validation error: {}", err);
            Err((StatusCode::BAD_REQUEST, format!("Error: {err}")))
        }
    }
}

async fn preview_file(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<Html<String>, (StatusCode, String)> {
    let file_path = params.get("path").ok_or((
        StatusCode::BAD_REQUEST,
        "Missing path parameter".to_string(),
    ))?;

    // Don't preview markdown or image files with this handler
    if is_markdown_file(file_path) || is_image_file(file_path) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Use specific handlers for markdown and image files".to_string(),
        ));
    }

    match validate_file_path(&state.target_dir, file_path) {
        Ok(full_path) => {
            let csrf_token = generate_csrf_token(&state.csrf_secret);
            match get_file_size(&full_path) {
                Ok(size_bytes) => {
                    let file_size = format_file_size(size_bytes);
                    match generate_file_preview_html(file_path, &file_size, &csrf_token) {
                        Ok(html) => Ok(Html(html)),
                        Err(err) => {
                            warn!("Template render error: {}", err);
                            Err((StatusCode::INTERNAL_SERVER_ERROR, err))
                        }
                    }
                }
                Err(err) => {
                    warn!("Failed to get file size: {}", err);
                    // Fall back to generating without size info
                    match generate_file_preview_html(file_path, "Unknown", &csrf_token) {
                        Ok(html) => Ok(Html(html)),
                        Err(render_err) => {
                            warn!("Template render error: {}", render_err);
                            Err((StatusCode::INTERNAL_SERVER_ERROR, render_err))
                        }
                    }
                }
            }
        }
        Err(err) => {
            warn!("File validation error: {}", err);
            Err((StatusCode::BAD_REQUEST, format!("Error: {err}")))
        }
    }
}

async fn serve_file(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    let file_path = params.get("path").ok_or((
        StatusCode::BAD_REQUEST,
        "Missing path parameter".to_string(),
    ))?;

    // Only serve safe files
    if !is_safe_for_iframe(file_path) || is_executable_file(file_path) {
        return Err((
            StatusCode::FORBIDDEN,
            "File type not allowed for security reasons".to_string(),
        ));
    }

    match validate_file_path(&state.target_dir, file_path) {
        Ok(full_path) => {
            match fs::read(&full_path) {
                Ok(file_contents) => {
                    // Determine content type based on file extension
                    let content_type = match full_path.extension().and_then(|s| s.to_str()) {
                        Some("txt") | Some("log") => "text/plain; charset=utf-8",
                        Some("html") | Some("htm") => "text/html; charset=utf-8",
                        Some("css") => "text/css; charset=utf-8",
                        Some("js") => "application/javascript; charset=utf-8",
                        Some("json") => "application/json; charset=utf-8",
                        Some("xml") => "application/xml; charset=utf-8",
                        Some("pdf") => "application/pdf",
                        Some("csv") => "text/csv; charset=utf-8",
                        Some("yml") | Some("yaml") => "text/yaml; charset=utf-8",
                        Some("toml") => "text/plain; charset=utf-8",
                        Some("ini") | Some("conf") | Some("cfg") => "text/plain; charset=utf-8",
                        _ => "text/plain; charset=utf-8",
                    };

                    Ok(axum::response::Response::builder()
                        .header("Content-Type", content_type)
                        .header("X-Content-Type-Options", "nosniff")
                        .header("X-Frame-Options", "SAMEORIGIN")
                        .body(axum::body::Body::from(file_contents))
                        .unwrap())
                }
                Err(err) => {
                    warn!("File read error: {}", err);
                    Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Error reading file: {err}"),
                    ))
                }
            }
        }
        Err(err) => {
            warn!("File validation error: {}", err);
            Err((StatusCode::BAD_REQUEST, format!("Error: {err}")))
        }
    }
}

async fn get_file_info(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<Json<FileInfo>, (StatusCode, String)> {
    let file_path = params.get("path").ok_or((
        StatusCode::BAD_REQUEST,
        "Missing path parameter".to_string(),
    ))?;

    match validate_file_path(&state.target_dir, file_path) {
        Ok(full_path) => {
            match (
                get_file_modification_time(&full_path),
                get_file_size(&full_path),
            ) {
                (Ok(modified_time), Ok(size)) => {
                    let file_info = FileInfo {
                        modified_time,
                        size,
                    };
                    Ok(Json(file_info))
                }
                (Err(e), _) | (_, Err(e)) => {
                    warn!("Failed to get file info: {}", e);
                    Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Error getting file info: {e}"),
                    ))
                }
            }
        }
        Err(err) => {
            warn!("File validation error: {}", err);
            Err((StatusCode::BAD_REQUEST, format!("Error: {err}")))
        }
    }
}

async fn get_file_content(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<Json<FileContent>, (StatusCode, String)> {
    let file_path = params.get("path").ok_or((
        StatusCode::BAD_REQUEST,
        "Missing path parameter".to_string(),
    ))?;

    // Only allow markdown files for this endpoint
    if !is_markdown_file(file_path) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Only markdown files are supported".to_string(),
        ));
    }

    match validate_file_path(&state.target_dir, file_path) {
        Ok(full_path) => {
            match (
                read_file_content(&full_path),
                get_file_modification_time(&full_path),
            ) {
                (Ok(content), Ok(modified_time)) => {
                    let file_content = FileContent {
                        content,
                        modified_time,
                    };
                    Ok(Json(file_content))
                }
                (Err(e), _) | (_, Err(e)) => {
                    warn!("Failed to get file content: {}", e);
                    Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Error getting file content: {e}"),
                    ))
                }
            }
        }
        Err(err) => {
            warn!("File validation error: {}", err);
            Err((StatusCode::BAD_REQUEST, format!("Error: {err}")))
        }
    }
}

async fn delete_file(
    State(state): State<AppState>,
    Form(form): Form<DeleteForm>,
) -> Result<Html<String>, (StatusCode, String)> {
    // Validate CSRF token
    if !validate_csrf_token(&form.csrf_token, &state.csrf_secret) {
        return Err((StatusCode::FORBIDDEN, "Invalid CSRF token".to_string()));
    }

    // Validate the file path
    match validate_file_path(&state.target_dir, &form.path) {
        Ok(full_path) => match fs::remove_file(&full_path) {
            Ok(()) => {
                info!("File deleted successfully: {}", form.path);
                let parent_path = get_parent_directory_path(&form.path);
                match generate_status_html(StatusPageContext {
                    title: "File Deleted - Markdown Wrangler",
                    heading: "🗑️ File Deleted Successfully!",
                    heading_class: "success",
                    file_path: &form.path,
                    detail_text: "has been deleted.",
                    show_edit_button: false,
                    edit_url: "",
                    back_url: &parent_path,
                }) {
                    Ok(html) => Ok(Html(html)),
                    Err(err) => {
                        warn!("Template render error: {}", err);
                        Err((StatusCode::INTERNAL_SERVER_ERROR, err))
                    }
                }
            }
            Err(err) => {
                warn!("File deletion error: {}", err);
                Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Error deleting file: {err}"),
                ))
            }
        },
        Err(err) => {
            warn!("File validation error during deletion: {}", err);
            Err((StatusCode::BAD_REQUEST, format!("Error: {err}")))
        }
    }
}

async fn handler_404() -> (StatusCode, &'static str) {
    (StatusCode::NOT_FOUND, "not found")
}

fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/edit", get(edit_file))
        .route("/save", post(save_file))
        .route("/delete", post(delete_file))
        .route("/preview", get(preview_image))
        .route("/image", get(serve_image))
        .route("/file-preview", get(preview_file))
        .route("/file", get(serve_file))
        .route("/file-info", get(get_file_info))
        .route("/file-content", get(get_file_content))
        .nest_service("/static", ServeDir::new("static"))
        .fallback(handler_404)
        .with_state(state)
}

pub async fn start_server(target_dir: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    // Generate a random CSRF secret
    let csrf_secret = hex::encode(rand::rng().random::<[u8; 32]>());
    let state = AppState {
        target_dir,
        csrf_secret,
    };
    let app = create_router(state);

    let address = "127.0.0.1:5420";
    let listener = TcpListener::bind(address).await?;
    info!(
        "Web server listening on http://{}, press Ctrl+C to stop",
        address
    );

    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Method, Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use tempfile::TempDir;
    use tower::ServiceExt;

    async fn create_test_app() -> (Router, TempDir, String) {
        let temp_dir = TempDir::new().unwrap();
        let csrf_secret = "test_secret_key_for_csrf_testing".to_string();
        let state = AppState {
            target_dir: temp_dir.path().to_path_buf(),
            csrf_secret: csrf_secret.clone(),
        };
        let app = create_router(state);
        (app, temp_dir, csrf_secret)
    }

    fn create_expired_csrf_token(secret: &str) -> String {
        let expired_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 7200; // 2 hours ago
        let payload = format!("{expired_timestamp}:12345");
        let mut hasher = Sha256::new();
        hasher.update(payload.as_bytes());
        hasher.update(secret.as_bytes());
        let signature = hex::encode(hasher.finalize());
        format!("{payload}:{signature}")
    }

    #[test]
    fn test_csrf_token_generation() {
        let secret = "test_secret";
        let token = generate_csrf_token(secret);

        // Token should have 3 parts separated by colons
        let parts: Vec<&str> = token.split(':').collect();
        assert_eq!(parts.len(), 3);

        // All parts should be non-empty
        assert!(!parts[0].is_empty()); // timestamp
        assert!(!parts[1].is_empty()); // nonce
        assert!(!parts[2].is_empty()); // signature
    }

    #[test]
    fn test_csrf_token_validation_valid() {
        let secret = "test_secret";
        let token = generate_csrf_token(secret);

        // Valid token should pass validation
        assert!(validate_csrf_token(&token, secret));
    }

    #[test]
    fn test_csrf_token_validation_wrong_secret() {
        let secret = "test_secret";
        let wrong_secret = "wrong_secret";
        let token = generate_csrf_token(secret);

        // Token with wrong secret should fail validation
        assert!(!validate_csrf_token(&token, wrong_secret));
    }

    #[test]
    fn test_csrf_token_validation_malformed() {
        let secret = "test_secret";

        // Various malformed tokens should fail validation
        assert!(!validate_csrf_token("", secret));
        assert!(!validate_csrf_token("invalid", secret));
        assert!(!validate_csrf_token("only:two", secret));
        assert!(!validate_csrf_token("too:many:parts:here", secret));
        assert!(!validate_csrf_token(":::", secret));
    }

    #[test]
    fn test_csrf_token_validation_expired() {
        let secret = "test_secret";

        // Create an expired token (timestamp from 2 hours ago)
        let expired_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 7200; // 2 hours ago

        let payload = format!("{expired_timestamp}:12345");
        let mut hasher = Sha256::new();
        hasher.update(payload.as_bytes());
        hasher.update(secret.as_bytes());
        let signature = hex::encode(hasher.finalize());
        let expired_token = format!("{payload}:{signature}");

        // Expired token should fail validation
        assert!(!validate_csrf_token(&expired_token, secret));
    }

    #[test]
    fn test_csrf_token_validation_invalid_timestamp() {
        let secret = "test_secret";

        // Token with invalid timestamp should fail validation
        let invalid_token = "invalid_timestamp:12345:signature";
        assert!(!validate_csrf_token(invalid_token, secret));
    }

    #[tokio::test]
    async fn test_save_endpoint_without_csrf_token() {
        let (app, temp_dir, _) = create_test_app().await;

        // Create a test markdown file
        let test_file = temp_dir.path().join("test.md");
        std::fs::write(&test_file, "# Test").unwrap();

        // Try to save without CSRF token
        let request = Request::builder()
            .method(Method::POST)
            .uri("/save")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from("path=test.md&content=# Updated Content"))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // Should return 422 Unprocessable Entity due to missing CSRF token
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_save_endpoint_with_invalid_csrf_token() {
        let (app, temp_dir, _) = create_test_app().await;

        // Create a test markdown file
        let test_file = temp_dir.path().join("test.md");
        std::fs::write(&test_file, "# Test").unwrap();

        // Try to save with invalid CSRF token
        let request = Request::builder()
            .method(Method::POST)
            .uri("/save")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(
                "path=test.md&content=# Updated Content&csrf_token=invalid",
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // Should return 403 Forbidden due to invalid CSRF token
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body_text = String::from_utf8(body.to_vec()).unwrap();
        assert!(body_text.contains("Invalid CSRF token"));

        // File should remain unchanged
        let content = std::fs::read_to_string(&test_file).unwrap();
        assert_eq!(content, "# Test");
    }

    #[tokio::test]
    async fn test_save_endpoint_with_expired_csrf_token() {
        let (app, temp_dir, csrf_secret) = create_test_app().await;

        let test_file = temp_dir.path().join("test.md");
        std::fs::write(&test_file, "# Test").unwrap();

        let expired_token = create_expired_csrf_token(&csrf_secret);
        let body = format!(
            "path=test.md&content=# Updated Content&csrf_token={}",
            urlencoding::encode(&expired_token)
        );

        let request = Request::builder()
            .method(Method::POST)
            .uri("/save")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body_text = String::from_utf8(body.to_vec()).unwrap();
        assert!(body_text.contains("Invalid CSRF token"));

        // File should remain unchanged
        let content = std::fs::read_to_string(&test_file).unwrap();
        assert_eq!(content, "# Test");
    }

    #[tokio::test]
    async fn test_save_endpoint_with_valid_csrf_token() {
        let (app, temp_dir, csrf_secret) = create_test_app().await;

        // Create a test markdown file
        let test_file = temp_dir.path().join("test.md");
        std::fs::write(&test_file, "# Test").unwrap();

        // Generate valid CSRF token
        let csrf_token = generate_csrf_token(&csrf_secret);

        // Save with valid CSRF token
        let body = format!(
            "path=test.md&content=# Updated Content&csrf_token={}",
            urlencoding::encode(&csrf_token)
        );

        let request = Request::builder()
            .method(Method::POST)
            .uri("/save")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // Should succeed
        assert_eq!(response.status(), StatusCode::OK);

        // Verify file content was updated
        let content = std::fs::read_to_string(&test_file).unwrap();
        assert_eq!(content, "# Updated Content");
    }

    #[tokio::test]
    async fn test_delete_endpoint_without_csrf_token() {
        let (app, temp_dir, _) = create_test_app().await;

        // Create a test markdown file
        let test_file = temp_dir.path().join("test.md");
        std::fs::write(&test_file, "# Test").unwrap();

        // Try to delete without CSRF token
        let request = Request::builder()
            .method(Method::POST)
            .uri("/delete")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from("path=test.md"))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // Should return 422 Unprocessable Entity due to missing CSRF token
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

        // File should still exist
        assert!(test_file.exists());
    }

    #[tokio::test]
    async fn test_delete_endpoint_with_valid_csrf_token() {
        let (app, temp_dir, csrf_secret) = create_test_app().await;

        // Create a test markdown file
        let test_file = temp_dir.path().join("test.md");
        std::fs::write(&test_file, "# Test").unwrap();

        // Generate valid CSRF token
        let csrf_token = generate_csrf_token(&csrf_secret);

        // Delete with valid CSRF token
        let body = format!(
            "path=test.md&csrf_token={}",
            urlencoding::encode(&csrf_token)
        );

        let request = Request::builder()
            .method(Method::POST)
            .uri("/delete")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // Should succeed
        assert_eq!(response.status(), StatusCode::OK);

        // File should be deleted
        assert!(!test_file.exists());
    }

    #[tokio::test]
    async fn test_delete_endpoint_with_invalid_csrf_token() {
        let (app, temp_dir, _) = create_test_app().await;

        let test_file = temp_dir.path().join("test.md");
        std::fs::write(&test_file, "# Test").unwrap();

        let request = Request::builder()
            .method(Method::POST)
            .uri("/delete")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from("path=test.md&csrf_token=invalid"))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body_text = String::from_utf8(body.to_vec()).unwrap();
        assert!(body_text.contains("Invalid CSRF token"));

        // File should still exist
        assert!(test_file.exists());
    }

    #[tokio::test]
    async fn test_delete_endpoint_with_expired_csrf_token() {
        let (app, temp_dir, csrf_secret) = create_test_app().await;

        let test_file = temp_dir.path().join("test.md");
        std::fs::write(&test_file, "# Test").unwrap();

        let expired_token = create_expired_csrf_token(&csrf_secret);
        let body = format!(
            "path=test.md&csrf_token={}",
            urlencoding::encode(&expired_token)
        );

        let request = Request::builder()
            .method(Method::POST)
            .uri("/delete")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body_text = String::from_utf8(body.to_vec()).unwrap();
        assert!(body_text.contains("Invalid CSRF token"));

        // File should still exist
        assert!(test_file.exists());
    }

    #[tokio::test]
    async fn test_edit_page_contains_csrf_token() {
        let (app, temp_dir, _) = create_test_app().await;

        // Create a test markdown file
        let test_file = temp_dir.path().join("test.md");
        std::fs::write(&test_file, "# Test Content").unwrap();

        // Request the edit page
        let request = Request::builder()
            .method(Method::GET)
            .uri("/edit?path=test.md")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Get response body
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let html = String::from_utf8(body.to_vec()).unwrap();

        // Verify CSRF token is present in both forms
        assert!(html.contains(r#"name="csrf_token""#));

        // Should have at least 2 CSRF token fields (save form and delete form)
        let csrf_count = html.matches(r#"name="csrf_token""#).count();
        assert_eq!(csrf_count, 2);
    }

    #[tokio::test]
    async fn test_image_preview_page_contains_csrf_token() {
        let (app, temp_dir, _) = create_test_app().await;

        let test_file = temp_dir.path().join("image.png");
        std::fs::write(&test_file, "fake image data").unwrap();

        let request = Request::builder()
            .method(Method::GET)
            .uri("/preview?path=image.png")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let html = String::from_utf8(body.to_vec()).unwrap();
        assert!(html.contains(r#"name="csrf_token""#));
    }

    #[tokio::test]
    async fn test_file_preview_page_contains_csrf_token() {
        let (app, temp_dir, _) = create_test_app().await;

        let test_file = temp_dir.path().join("notes.txt");
        std::fs::write(&test_file, "hello").unwrap();

        let request = Request::builder()
            .method(Method::GET)
            .uri("/file-preview?path=notes.txt")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let html = String::from_utf8(body.to_vec()).unwrap();
        assert!(html.contains(r#"name="csrf_token""#));
    }

    #[test]
    fn test_csrf_tokens_are_unique() {
        let secret = "test_secret";

        // Generate multiple tokens
        let token1 = generate_csrf_token(secret);
        let token2 = generate_csrf_token(secret);
        let token3 = generate_csrf_token(secret);

        // All tokens should be different (due to timestamp and nonce)
        assert_ne!(token1, token2);
        assert_ne!(token2, token3);
        assert_ne!(token1, token3);

        // But all should validate with the same secret
        assert!(validate_csrf_token(&token1, secret));
        assert!(validate_csrf_token(&token2, secret));
        assert!(validate_csrf_token(&token3, secret));
    }

    #[test]
    fn test_is_image_file() {
        // Test common image file extensions
        assert!(is_image_file("photo.jpg"));
        assert!(is_image_file("image.jpeg"));
        assert!(is_image_file("picture.png"));
        assert!(is_image_file("animation.gif"));
        assert!(is_image_file("modern.webp"));
        assert!(is_image_file("vector.svg"));
        assert!(is_image_file("bitmap.bmp"));
        assert!(is_image_file("professional.tiff"));
        assert!(is_image_file("scan.tif"));

        // Test case insensitivity
        assert!(is_image_file("PHOTO.JPG"));
        assert!(is_image_file("Image.PNG"));
        assert!(is_image_file("Vector.SVG"));

        // Test mixed case
        assert!(is_image_file("MyPhoto.JpEg"));
        assert!(is_image_file("screenshot.Png"));

        // Test with paths
        assert!(is_image_file("assets/images/photo.jpg"));
        assert!(is_image_file("/home/user/pictures/vacation.png"));
        assert!(is_image_file("../images/logo.svg"));

        // Test non-image files
        assert!(!is_image_file("document.txt"));
        assert!(!is_image_file("script.js"));
        assert!(!is_image_file("style.css"));
        assert!(!is_image_file("data.json"));
        assert!(!is_image_file("README.md"));
        assert!(!is_image_file("config.toml"));

        // Test files without extensions
        assert!(!is_image_file("filename"));
        assert!(!is_image_file("no_extension"));

        // Test empty string and edge cases
        assert!(!is_image_file(""));
        assert!(!is_image_file("."));
        assert!(!is_image_file(".."));
        assert!(!is_image_file(".hidden"));

        // Test partial matches that shouldn't work
        assert!(!is_image_file("jpgfile.txt"));
        assert!(!is_image_file("not_png_file.doc"));
    }

    #[test]
    fn test_is_executable_file() {
        // Test Windows executable extensions
        assert!(is_executable_file("program.exe"));
        assert!(is_executable_file("script.bat"));
        assert!(is_executable_file("command.cmd"));
        assert!(is_executable_file("old_program.com"));
        assert!(is_executable_file("screensaver.scr"));
        assert!(is_executable_file("installer.msi"));

        // Test Unix/Linux executable extensions
        assert!(is_executable_file("script.sh"));

        // Test PowerShell and other script extensions
        assert!(is_executable_file("automation.ps1"));
        assert!(is_executable_file("legacy.vbs"));

        // Test macOS executable extensions
        assert!(is_executable_file("Application.app"));
        assert!(is_executable_file("disk_image.dmg"));
        assert!(is_executable_file("package.pkg"));

        // Test Linux package formats
        assert!(is_executable_file("package.deb"));
        assert!(is_executable_file("redhat.rpm"));

        // Test case insensitivity
        assert!(is_executable_file("PROGRAM.EXE"));
        assert!(is_executable_file("Script.SH"));
        assert!(is_executable_file("Package.DEB"));

        // Test with paths
        assert!(is_executable_file("bin/program.exe"));
        assert!(is_executable_file("/usr/local/bin/script.sh"));
        assert!(is_executable_file("../downloads/installer.msi"));

        // Test non-executable files
        assert!(!is_executable_file("document.txt"));
        assert!(!is_executable_file("image.jpg"));
        assert!(!is_executable_file("script.js"));
        assert!(!is_executable_file("style.css"));
        assert!(!is_executable_file("README.md"));
        assert!(!is_executable_file("config.toml"));

        // Test files without extensions
        assert!(!is_executable_file("filename"));
        assert!(!is_executable_file("no_extension"));

        // Test empty string and edge cases
        assert!(!is_executable_file(""));
        assert!(!is_executable_file("."));
        assert!(!is_executable_file(".."));
        assert!(!is_executable_file(".hidden"));

        // Test partial matches that shouldn't work
        assert!(!is_executable_file("exefile.txt"));
        assert!(!is_executable_file("not_bat_file.doc"));
    }

    #[test]
    fn test_is_safe_for_iframe() {
        // Test text files
        assert!(is_safe_for_iframe("document.txt"));
        assert!(is_safe_for_iframe("README.txt"));
        assert!(is_safe_for_iframe("notes.log"));

        // Test web files
        assert!(is_safe_for_iframe("index.html"));
        assert!(is_safe_for_iframe("page.htm"));
        assert!(is_safe_for_iframe("styles.css"));
        assert!(is_safe_for_iframe("script.js"));

        // Test data files
        assert!(is_safe_for_iframe("data.json"));
        assert!(is_safe_for_iframe("config.xml"));
        assert!(is_safe_for_iframe("spreadsheet.csv"));

        // Test configuration files
        assert!(is_safe_for_iframe("config.yml"));
        assert!(is_safe_for_iframe("settings.yaml"));
        assert!(is_safe_for_iframe("Cargo.toml"));
        assert!(is_safe_for_iframe("config.ini"));
        assert!(is_safe_for_iframe("app.conf"));
        assert!(is_safe_for_iframe("settings.cfg"));

        // Test documents
        assert!(is_safe_for_iframe("manual.pdf"));

        // Test case insensitivity
        assert!(is_safe_for_iframe("DOCUMENT.TXT"));
        assert!(is_safe_for_iframe("Index.HTML"));
        assert!(is_safe_for_iframe("Config.JSON"));
        assert!(is_safe_for_iframe("Settings.YAML"));

        // Test with paths
        assert!(is_safe_for_iframe("docs/readme.txt"));
        assert!(is_safe_for_iframe("/var/log/system.log"));
        assert!(is_safe_for_iframe("../config/app.toml"));

        // Test unsafe file types
        assert!(!is_safe_for_iframe("program.exe"));
        assert!(!is_safe_for_iframe("script.bat"));
        assert!(!is_safe_for_iframe("image.jpg"));
        assert!(!is_safe_for_iframe("archive.zip"));
        assert!(!is_safe_for_iframe("binary.bin"));
        assert!(!is_safe_for_iframe("unknown.xyz"));

        // Test files without extensions
        assert!(!is_safe_for_iframe("filename"));
        assert!(!is_safe_for_iframe("no_extension"));

        // Test empty string and edge cases
        assert!(!is_safe_for_iframe(""));
        assert!(!is_safe_for_iframe("."));
        assert!(!is_safe_for_iframe(".."));
        assert!(!is_safe_for_iframe(".hidden"));

        // Test partial matches that shouldn't work
        assert!(!is_safe_for_iframe("txtfile.exe"));
        assert!(!is_safe_for_iframe("not_html_file.doc"));
    }
}
