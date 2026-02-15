// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

pub(crate) mod constants;
pub mod error;

use askama::Template;
use askama_web::WebTemplate;
use axum::{
    Router,
    extract::{Form, Query, State},
    http::HeaderValue,
    response::{Json, Redirect, Response},
    routing::{get, post},
};
use constants::*;
use hmac::{Hmac, Mac};

use rand::RngExt;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::fs;
use tokio::net::TcpListener;
use tower_http::services::ServeDir;
use tracing::{debug, info, warn};

use crate::web::error::WebError;

type HmacSha256 = Hmac<Sha256>;

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

#[derive(Template, WebTemplate)]
#[template(path = "directory.html")]
struct DirectoryTemplate {
    at_root: bool,
    breadcrumbs: Vec<Breadcrumb>,
    has_parent: bool,
    parent_url: String,
    new_file_url: String,
    entries: Vec<DirectoryEntryView>,
}

#[derive(Template, WebTemplate)]
#[template(path = "editor.html")]
struct EditorTemplate {
    file_path: String,
    content: String,
    csrf_token: String,
    is_draft: bool,
}

#[derive(Template, WebTemplate)]
#[template(path = "image_preview.html")]
struct ImagePreviewTemplate {
    file_path: String,
    encoded_path: String,
    file_size: String,
    parent_path: String,
    csrf_token: String,
}

#[derive(Template, WebTemplate)]
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

#[derive(Template, WebTemplate)]
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

#[derive(Template, WebTemplate)]
#[template(path = "new_file.html")]
struct NewFileTemplate {
    current_path_display: String,
    path_value: String,
    back_url: String,
    csrf_token: String,
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

#[derive(Deserialize)]
struct NewFileForm {
    path: String,
    filename: String,
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
    let timestamp = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(err) => {
            warn!(
                "System time is before UNIX_EPOCH while generating CSRF token: {}",
                err
            );
            0
        }
    };
    let nonce: u64 = rand::rng().random();

    let payload = format!("{timestamp}:{nonce}");
    let signature = match HmacSha256::new_from_slice(secret.as_bytes()) {
        Ok(mut mac) => {
            mac.update(payload.as_bytes());
            hex::encode(mac.finalize().into_bytes())
        }
        Err(err) => {
            warn!(
                "Failed to initialize HMAC for CSRF token generation: {}",
                err
            );
            String::new()
        }
    };

    format!("{payload}:{signature}")
}

pub(crate) fn validate_csrf_token(token: &str, secret: &str) -> Result<(), WebError> {
    let parts: Vec<&str> = token.split(':').collect();

    let [timestamp_str, nonce, provided_signature] = parts.as_slice() else {
        return Err(WebError::Forbidden("Invalid CSRF Token".to_string()));
    };

    // Check if token is not too old (1 hour)
    if let Ok(timestamp) = timestamp_str.parse::<u64>() {
        let current_time = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_secs(),
            Err(err) => {
                warn!(
                    "System time is before UNIX_EPOCH while validating CSRF token: {}",
                    err
                );
                return Err(WebError::Forbidden("Invalid CSRF Token".to_string()));
            }
        };

        if current_time.saturating_sub(timestamp) > 3600 {
            debug!(?timestamp, ?current_time, "CSRF token expired");
            return Err(WebError::Forbidden("Invalid CSRF Token".to_string()));
        }
    } else {
        return Err(WebError::Forbidden("Invalid CSRF Token".to_string()));
    }

    let Ok(signature_bytes) = hex::decode(provided_signature) else {
        return Err(WebError::Forbidden("Invalid CSRF Token".to_string()));
    };

    let payload = format!("{timestamp_str}:{nonce}");
    let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
        Ok(mac) => mac,
        Err(err) => {
            warn!(
                "Failed to initialize HMAC for CSRF token validation: {}",
                err
            );
            return Err(WebError::Forbidden("Invalid CSRF Token".to_string()));
        }
    };
    mac.update(payload.as_bytes());

    if mac.verify_slice(&signature_bytes).is_ok() {
        Ok(())
    } else {
        Err(WebError::Forbidden("Invalid CSRF Token".to_string()))
    }
}

async fn list_directory(
    base_dir: &Path,
    relative_path: &str,
) -> Result<Vec<DirectoryEntry>, WebError> {
    let full_path = if relative_path.is_empty() {
        base_dir.to_path_buf()
    } else {
        base_dir.join(relative_path)
    };

    // Security check: ensure the path is within the base directory
    let canonical_base = base_dir
        .canonicalize()
        .map_err(|e| WebError::BadRequest(format!("Base directory error: {e}")))?;
    let canonical_full = full_path
        .canonicalize()
        .map_err(|e| WebError::BadRequest(format!("Path error: {e}")))?;

    if !canonical_full.starts_with(&canonical_base) {
        warn!(
            "Directory traversal attempt detected: {}",
            full_path.display()
        );
        return Err(WebError::Unauthorized);
    }

    let mut entries = fs::read_dir(&full_path).await?;

    let mut directory_entries = Vec::new();

    while let Some(entry) = entries.next_entry().await? {
        let file_name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden files
        if file_name.starts_with('.') {
            continue;
        }

        let is_directory = entry.file_type().await?.is_dir();

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

fn validate_file_path(base_dir: &Path, relative_path: &str) -> Result<PathBuf, WebError> {
    let full_path = base_dir.join(relative_path);

    // Security check: ensure the path is within the base directory
    let canonical_base = base_dir
        .canonicalize()
        .map_err(|e| WebError::BadRequest(format!("Base directory error: {e}")))?;
    let canonical_full = full_path
        .canonicalize()
        .map_err(|_| WebError::BadRequest("Path does not exist".to_string()))?;

    if !canonical_full.starts_with(&canonical_base) {
        return Err(WebError::BadRequest(
            "Path outside base directory".to_string(),
        ));
    }

    if !canonical_full.is_file() {
        return Err(WebError::BadRequest("Path is not a file".to_string()));
    }

    Ok(canonical_full)
}

fn validate_directory_path(base_dir: &Path, relative_path: &str) -> Result<PathBuf, WebError> {
    let full_path = if relative_path.is_empty() {
        base_dir.to_path_buf()
    } else {
        base_dir.join(relative_path)
    };

    let canonical_base = base_dir
        .canonicalize()
        .map_err(|e| WebError::BadRequest(format!("Base directory error: {e}")))?;
    let canonical_full = full_path
        .canonicalize()
        .map_err(|_| WebError::BadRequest("Path does not exist".to_string()))?;

    if !canonical_full.starts_with(&canonical_base) {
        return Err(WebError::BadRequest(
            "Path outside base directory".to_string(),
        ));
    }

    if !canonical_full.is_dir() {
        return Err(WebError::BadRequest("Path is not a directory".to_string()));
    }

    Ok(canonical_full)
}

fn is_git_compatible_ascii_filename_stem(stem: &str) -> bool {
    !stem.is_empty()
        && stem.is_ascii()
        && !stem.starts_with('.')
        && !stem.ends_with('.')
        && !stem.contains("..")
        && stem
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

fn normalize_markdown_filename(filename: &str) -> Result<String, WebError> {
    let trimmed = filename.trim();
    if trimmed.is_empty() {
        return Err(WebError::BadRequest("Filename is required".to_string()));
    }

    let mut stem = trimmed.to_string();
    let lower = stem.to_ascii_lowercase();
    if lower.ends_with(".markdown") {
        stem.truncate(stem.len().saturating_sub(".markdown".len()));
    } else if lower.ends_with(".md") {
        stem.truncate(stem.len().saturating_sub(".md".len()));
    }
    let stem = stem.trim();

    if !is_git_compatible_ascii_filename_stem(stem) {
        return Err(WebError::BadRequest(
            "Filename must use only ASCII letters, numbers, '-', '_', or '.'".to_string(),
        ));
    }

    Ok(format!("{stem}.md"))
}

fn is_markdown_file(path: &str) -> bool {
    path.to_lowercase().ends_with(".md") || path.to_lowercase().ends_with(".markdown")
}

#[derive(Clone, Copy)]
enum FrontmatterFormat {
    Yaml,
    Json,
}

type ParsedFrontmatter = (
    Option<bool>,
    Option<String>,
    Option<String>,
    Vec<String>,
    Vec<String>,
    HashMap<String, serde_json::Value>,
);

fn extract_yaml_frontmatter(content: &str) -> Option<&str> {
    let start = if content.starts_with("---\n") {
        4
    } else if content.starts_with("---\r\n") {
        5
    } else {
        return None;
    };

    let rest = &content[start..];
    let mut offset = start;
    for line in rest.split('\n') {
        if line.trim_end_matches('\r').trim() == "---" {
            return Some(&content[start..offset]);
        }

        offset = std::cmp::min(content.len(), offset + line.len() + 1);
    }

    None
}

fn extract_json_frontmatter(content: &str) -> Option<&str> {
    if !content.starts_with('{') {
        return None;
    }

    let mut depth = 0usize;
    let mut in_string = false;
    let mut escape_next = false;

    for (idx, ch) in content.char_indices() {
        if in_string {
            if escape_next {
                escape_next = false;
            } else if ch == '\\' {
                escape_next = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => depth = depth.saturating_add(1),
            '}' => {
                if depth == 0 {
                    return None;
                }

                depth -= 1;
                if depth == 0 {
                    let object_end = idx + ch.len_utf8();
                    let remainder = &content[object_end..];

                    if remainder.is_empty() {
                        return Some(&content[..object_end]);
                    }

                    let after_ws = remainder.trim_start_matches([' ', '\t', '\r']);
                    if after_ws.starts_with('\n') {
                        return Some(&content[..object_end]);
                    }

                    return None;
                }
            }
            _ => {}
        }
    }

    None
}

fn extract_frontmatter(content: &str) -> Option<(FrontmatterFormat, &str)> {
    if let Some(frontmatter) = extract_yaml_frontmatter(content) {
        return Some((FrontmatterFormat::Yaml, frontmatter));
    }

    if let Some(frontmatter) = extract_json_frontmatter(content) {
        return Some((FrontmatterFormat::Json, frontmatter));
    }

    None
}

fn parse_bool_value(value: &serde_json::Value) -> Option<bool> {
    if let Some(boolean) = value.as_bool() {
        return Some(boolean);
    }

    if let Some(text) = value.as_str() {
        let normalized = text.trim().to_ascii_lowercase();
        return match normalized.as_str() {
            "true" | "yes" | "on" | "1" => Some(true),
            "false" | "no" | "off" | "0" => Some(false),
            _ => None,
        };
    }

    if let Some(number) = value.as_i64() {
        return match number {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        };
    }

    if let Some(number) = value.as_u64() {
        return match number {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        };
    }

    None
}

fn parse_string_value(value: &serde_json::Value) -> Option<String> {
    let maybe_string = if let Some(text) = value.as_str() {
        Some(text.trim().to_string())
    } else if let Some(number) = value.as_i64() {
        Some(number.to_string())
    } else if let Some(number) = value.as_u64() {
        Some(number.to_string())
    } else if let Some(number) = value.as_f64() {
        Some(number.to_string())
    } else {
        value.as_bool().map(|boolean| boolean.to_string())
    };

    maybe_string.filter(|text| !text.is_empty())
}

fn parse_string_list_value(value: &serde_json::Value) -> Vec<String> {
    if let Some(items) = value.as_array() {
        return items.iter().filter_map(parse_string_value).collect();
    }

    if let Some(text) = value.as_str() {
        if text.contains(',') {
            return text
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect();
        }

        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return vec![trimmed.to_string()];
        }

        return Vec::new();
    }

    parse_string_value(value).map_or_else(Vec::new, |item| vec![item])
}

fn parse_frontmatter(content: &str) -> Option<ParsedFrontmatter> {
    let (format, frontmatter) = extract_frontmatter(content)?;

    let parsed_value = match format {
        FrontmatterFormat::Yaml => serde_yaml::from_str::<serde_json::Value>(frontmatter).ok()?,
        FrontmatterFormat::Json => serde_json::from_str::<serde_json::Value>(frontmatter).ok()?,
    };

    let serde_json::Value::Object(mut object) = parsed_value else {
        return None;
    };

    let draft = object.remove("draft").as_ref().and_then(parse_bool_value);
    let title = object.remove("title").as_ref().and_then(parse_string_value);
    let date = object.remove("date").as_ref().and_then(parse_string_value);
    let tags = object
        .remove("tags")
        .as_ref()
        .map_or_else(Vec::new, parse_string_list_value);
    let categories = object
        .remove("categories")
        .as_ref()
        .map_or_else(Vec::new, parse_string_list_value);
    let extra = object
        .into_iter()
        .collect::<HashMap<String, serde_json::Value>>();

    Some((draft, title, date, tags, categories, extra))
}

fn has_draft_frontmatter(content: &str) -> bool {
    if let Some((draft, _, _, _, _, _)) = parse_frontmatter(content) {
        return draft.unwrap_or(false);
    }

    false
}

async fn get_file_size(file_path: &Path) -> Result<u64, std::io::Error> {
    fs::metadata(file_path).await.map(|metadata| metadata.len())
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
        format!("{size_bytes} B")
    } else if let Some(unit) = UNITS.get(unit_index) {
        format!("{size:.1} {}", unit)
    } else {
        format!("{size_bytes} B")
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

async fn get_file_modification_time(file_path: &Path) -> Result<String, WebError> {
    fs::metadata(file_path)
        .await
        .and_then(|metadata| metadata.modified())
        .map(|time| {
            time.duration_since(SystemTime::UNIX_EPOCH)
                .map(|duration| duration.as_secs().to_string())
                .unwrap_or_else(|_| "0".to_string())
        })
        .map_err(|e| WebError::Internal(format!("Failed to get file modification time: {e}")))
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

async fn index(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<DirectoryTemplate, WebError> {
    let path = params.get("path").map(|s| s.as_str()).unwrap_or("");

    let entries = list_directory(&state.target_dir, path).await?;
    let parent_url = if let Some(pos) = path.rfind('/') {
        let parent_path = &path[..pos];
        if parent_path.is_empty() {
            "/".to_string()
        } else {
            format!("/?path={}", urlencoding::encode(parent_path))
        }
    } else {
        "/".to_string()
    };

    Ok(DirectoryTemplate {
        at_root: path.is_empty(),
        breadcrumbs: build_breadcrumbs(path),
        has_parent: !path.is_empty(),
        parent_url,
        new_file_url: if path.is_empty() {
            "/new-file".to_string()
        } else {
            format!("/new-file?path={}", urlencoding::encode(path))
        },
        entries: build_directory_entry_views(&entries),
    })
}

async fn new_file_form(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<NewFileTemplate, WebError> {
    let path = params.get("path").map(|s| s.as_str()).unwrap_or("");
    validate_directory_path(&state.target_dir, path)?;

    Ok(NewFileTemplate {
        current_path_display: if path.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", path)
        },
        path_value: path.to_string(),
        back_url: if path.is_empty() {
            "/".to_string()
        } else {
            format!("/?path={}", urlencoding::encode(path))
        },
        csrf_token: generate_csrf_token(&state.csrf_secret),
    })
}

async fn create_new_file(
    State(state): State<AppState>,
    Form(form): Form<NewFileForm>,
) -> Result<Redirect, WebError> {
    validate_csrf_token(&form.csrf_token, &state.csrf_secret)?;

    let canonical_dir = validate_directory_path(&state.target_dir, &form.path)?;
    let markdown_filename = normalize_markdown_filename(&form.filename)?;
    let full_path = canonical_dir.join(&markdown_filename);

    if fs::try_exists(&full_path).await? {
        return Err(WebError::BadRequest("File already exists".to_string()));
    }

    fs::write(&full_path, "").await?;

    let new_relative_path = if form.path.is_empty() {
        markdown_filename
    } else {
        format!("{}/{}", form.path, markdown_filename)
    };
    Ok(Redirect::to(&format!(
        "/edit?path={}",
        urlencoding::encode(&new_relative_path)
    )))
}

async fn edit_file(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<EditorTemplate, WebError> {
    let file_path = params
        .get("path")
        .ok_or(WebError::BadRequest("Missing path parameter".to_string()))?;

    if !is_markdown_file(file_path) {
        return Err(WebError::BadRequest(
            "File is not a markdown file".to_string(),
        ));
    }

    let full_path = validate_file_path(&state.target_dir, file_path)?;

    let content = fs::read_to_string(&full_path).await?;
    let is_draft = has_draft_frontmatter(&content);
    let csrf_token = generate_csrf_token(&state.csrf_secret);
    Ok(EditorTemplate {
        file_path: file_path.to_string(),
        content,
        csrf_token,
        is_draft,
    })
}

async fn save_file(
    State(state): State<AppState>,
    Form(form): Form<EditForm>,
) -> Result<StatusPageTemplate, WebError> {
    // Validate CSRF token
    validate_csrf_token(&form.csrf_token, &state.csrf_secret)?;

    if !is_markdown_file(&form.path) {
        return Err(WebError::BadRequest(
            "File is not a markdown file".to_string(),
        ));
    }

    let full_path = validate_file_path(&state.target_dir, &form.path)?;
    // Read existing content to check if it has changed
    let existing_content = fs::read_to_string(&full_path).await?;
    if existing_content == form.content {
        // Content hasn't changed, don't write to disk
        info!("File content unchanged, skipping write: {}", form.path);
        let back_url = get_parent_directory_path(&form.path);
        let edit_url = format!("/edit?path={}", urlencoding::encode(&form.path));
        Ok(StatusPageTemplate {
            title: "File Unchanged - Markdown Wrangler".to_string(),
            heading: "ℹ️ No Changes to Save".to_string(),
            heading_class: "success".to_string(),
            file_path: form.path,
            detail_text: "content is unchanged.".to_string(),
            show_edit_button: true,
            edit_url,
            back_url,
        })
    } else {
        // Content has changed, write to disk
        fs::write(&full_path, &form.content).await?;

        info!("File saved successfully: {}", form.path);
        let back_url = get_parent_directory_path(&form.path);
        let edit_url = format!("/edit?path={}", urlencoding::encode(&form.path));

        Ok(StatusPageTemplate {
            title: "File Saved - Markdown Wrangler".to_string(),
            heading: "✅ File Saved Successfully!".to_string(),
            heading_class: "success".to_string(),
            file_path: form.path.to_string(),
            detail_text: "has been saved.".to_string(),
            show_edit_button: true,
            edit_url,
            back_url,
        })
    }
}

async fn preview_image(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<ImagePreviewTemplate, WebError> {
    let file_path = params
        .get("path")
        .ok_or(WebError::BadRequest("Missing path parameter".to_string()))?
        .to_owned();

    if !is_image_file(&file_path) {
        return Err(WebError::BadRequest(
            "File is not an image file".to_string(),
        ));
    }

    let full_path = validate_file_path(&state.target_dir, &file_path)?;

    let csrf_token = generate_csrf_token(&state.csrf_secret);
    let parent_path = get_parent_directory_path(&file_path);
    let encoded_path = urlencoding::encode(&file_path).into_owned();
    let file_size = get_file_size(&full_path).await.map(format_file_size)?;
    Ok(ImagePreviewTemplate {
        encoded_path,
        parent_path,
        file_path,
        file_size,
        csrf_token,
    })
}

async fn serve_image(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<axum::response::Response, WebError> {
    let file_path = params
        .get("path")
        .ok_or(WebError::BadRequest("Missing path parameter".to_string()))?;

    if !is_image_file(file_path) {
        return Err(WebError::BadRequest(
            "File is not an image file".to_string(),
        ));
    }

    let full_path = validate_file_path(&state.target_dir, file_path)?;
    let file_contents = fs::read(&full_path).await?;
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

    let mut response = Response::new(axum::body::Body::from(file_contents));

    let header_value = match HeaderValue::from_str(content_type) {
        Ok(value) => value,
        Err(err) => {
            warn!(
                "Invalid content type '{}' for image response, using fallback: {}",
                content_type, err
            );
            HeaderValue::from_static("application/octet-stream")
        }
    };
    response.headers_mut().insert("Content-Type", header_value);

    Ok(response)
}

async fn preview_file(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<FilePreviewTemplate, WebError> {
    let file_path = params
        .get("path")
        .ok_or(WebError::BadRequest("Missing path parameter".to_string()))?;

    // Don't preview markdown or image files with this handler
    if is_markdown_file(file_path) || is_image_file(file_path) {
        return Err(WebError::BadRequest(
            "Use specific handlers for markdown and image files".to_string(),
        ));
    }

    let full_path = validate_file_path(&state.target_dir, file_path)?;
    let csrf_token = generate_csrf_token(&state.csrf_secret);
    match get_file_size(&full_path).await {
        Ok(size_bytes) => {
            let file_size = format_file_size(size_bytes);
            Ok(FilePreviewTemplate {
                file_path: file_path.to_string(),
                encoded_path: urlencoding::encode(file_path).into_owned(),
                file_size: file_size.to_string(),
                file_type: get_file_type_description(file_path).to_string(),
                parent_path: get_parent_directory_path(file_path),
                csrf_token: csrf_token.to_string(),
                can_iframe: is_safe_for_iframe(file_path),
            })
        }
        Err(err) => {
            warn!("Failed to get file size: {}", err);
            // Fall back to generating without size info
            Ok(FilePreviewTemplate {
                file_path: file_path.to_string(),
                encoded_path: urlencoding::encode(file_path).into_owned(),
                file_size: "Unknown".to_string(),
                file_type: get_file_type_description(file_path).to_string(),
                parent_path: get_parent_directory_path(file_path),
                csrf_token: csrf_token.to_string(),
                can_iframe: is_safe_for_iframe(file_path),
            })
        }
    }
}

async fn serve_file(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<axum::response::Response, WebError> {
    let file_path = params
        .get("path")
        .ok_or(WebError::BadRequest("Missing path parameter".to_string()))?;

    // Only serve safe files
    if !is_safe_for_iframe(file_path) || is_executable_file(file_path) {
        return Err(WebError::Forbidden(
            "File type not allowed for security reasons".to_string(),
        ));
    }

    let full_path = validate_file_path(&state.target_dir, file_path)?;

    let file_contents = fs::read(&full_path).await?;
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

    let mut response = axum::response::Response::new(axum::body::Body::from(file_contents));

    let headers = response.headers_mut();
    headers.insert("Content-Type", HeaderValue::from_static(content_type));

    headers.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("X-Frame-Options", HeaderValue::from_static("SAMEORIGIN"));

    Ok(response)
}

#[derive(Deserialize)]
pub struct ParamsWithPath {
    path: String,
}

async fn get_file_info(
    Query(params): Query<ParamsWithPath>,
    State(state): State<AppState>,
) -> Result<Json<FileInfo>, WebError> {
    let full_path = validate_file_path(&state.target_dir, &params.path)?;
    let modified_time = get_file_modification_time(&full_path).await?;
    let size = get_file_size(&full_path).await?;
    Ok(Json(FileInfo {
        modified_time,
        size,
    }))
}

async fn get_file_content(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<Json<FileContent>, WebError> {
    let file_path = params
        .get("path")
        .ok_or(WebError::BadRequest("Missing path parameter".to_string()))?;

    // Only allow markdown files for this endpoint
    if !is_markdown_file(file_path) {
        return Err(WebError::BadRequest(
            "Only markdown files are supported".to_string(),
        ));
    }

    let full_path = validate_file_path(&state.target_dir, file_path)?;
    let file_content = FileContent {
        content: fs::read_to_string(&full_path).await?,
        modified_time: get_file_modification_time(&full_path).await?,
    };
    Ok(Json(file_content))
}

async fn delete_file(
    State(state): State<AppState>,
    Form(form): Form<DeleteForm>,
) -> Result<StatusPageTemplate, WebError> {
    // Validate CSRF token
    validate_csrf_token(&form.csrf_token, &state.csrf_secret)?;

    // Validate the file path
    let full_path = validate_file_path(&state.target_dir, &form.path)?;
    fs::remove_file(&full_path).await?;
    info!("File deleted successfully: {}", form.path);
    let back_url = get_parent_directory_path(&form.path);
    Ok(StatusPageTemplate {
        title: "File Deleted - Markdown Wrangler".to_string(),
        heading: "🗑️ File Deleted Successfully!".to_string(),
        heading_class: "success".to_string(),
        file_path: form.path,
        detail_text: "has been deleted.".to_string(),
        show_edit_button: false,
        edit_url: "".to_string(),
        back_url,
    })
}

async fn handler_404() -> WebError {
    WebError::NotFound("Not found".to_string())
}

fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/new-file", get(new_file_form).post(create_new_file))
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
    use sha2::Digest;
    use tempfile::TempDir;
    use tower::ServiceExt;

    async fn create_test_app() -> (Router, TempDir, String) {
        let temp_dir = TempDir::new().expect("failed to create temporary test directory");
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
            .expect("system clock should be after UNIX_EPOCH")
            .as_secs()
            - 7200; // 2 hours ago
        let payload = format!("{expired_timestamp}:12345");
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
            .expect("failed to initialize HMAC for expired CSRF token");
        mac.update(payload.as_bytes());
        let signature = hex::encode(mac.finalize().into_bytes());
        format!("{payload}:{signature}")
    }

    fn create_legacy_keyed_csrf_token(secret: &str) -> String {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after UNIX_EPOCH")
            .as_secs();
        let payload = format!("{timestamp}:12345");
        let mut hasher = Sha256::new();
        hasher.update(payload.as_bytes());
        hasher.update(secret.as_bytes());
        let signature = hex::encode(hasher.finalize());
        format!("{payload}:{signature}")
    }

    fn extract_csrf_token_from_html(html: &str) -> Option<String> {
        let marker = r#"name="csrf_token" value=""#;
        let start = html.find(marker)? + marker.len();
        let remainder = &html[start..];
        let end = remainder.find('"')?;
        Some(remainder[..end].to_string())
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
        assert!(validate_csrf_token(&token, secret).is_ok());
    }

    #[test]
    fn test_csrf_token_validation_wrong_secret() {
        let secret = "test_secret";
        let wrong_secret = "wrong_secret";
        let token = generate_csrf_token(secret);

        // Token with wrong secret should fail validation
        assert!(validate_csrf_token(&token, wrong_secret).is_err());
    }

    #[test]
    fn test_csrf_token_validation_malformed() {
        let secret = "test_secret";

        // Various malformed tokens should fail validation
        assert!(validate_csrf_token("", secret).is_err());
        assert!(validate_csrf_token("invalid", secret).is_err());
        assert!(validate_csrf_token("only:two", secret).is_err());
        assert!(validate_csrf_token("too:many:parts:here", secret).is_err());
        assert!(validate_csrf_token(":::", secret).is_err());
        assert!(validate_csrf_token("1:2:not_hex_signature_@", secret).is_err());
    }

    #[test]
    fn test_csrf_token_validation_expired() {
        let secret = "test_secret";

        let expired_token = create_expired_csrf_token(secret);

        // Expired token should fail validation
        assert!(validate_csrf_token(&expired_token, secret).is_err());
    }

    #[test]
    fn test_csrf_token_validation_rejects_legacy_keyed_hash_token() {
        let secret = "test_secret";
        let legacy_token = create_legacy_keyed_csrf_token(secret);
        assert!(validate_csrf_token(&legacy_token, secret).is_err());
    }

    #[test]
    fn test_csrf_token_validation_invalid_timestamp() {
        let secret = "test_secret";

        // Token with invalid timestamp should fail validation
        let invalid_token = "invalid_timestamp:12345:signature";
        assert!(validate_csrf_token(invalid_token, secret).is_err());
    }

    #[test]
    fn test_has_draft_frontmatter_with_yaml_true() {
        let content = r#"---
title: Post
draft: true
---
# Hello
"#;
        assert!(has_draft_frontmatter(content));
    }

    #[test]
    fn test_has_draft_frontmatter_with_yaml_false() {
        let content = r#"---
title: Post
draft: false
---
# Hello
"#;
        assert!(!has_draft_frontmatter(content));
    }

    #[test]
    fn test_has_draft_frontmatter_with_json_true() {
        let content = r#"{
  "title": "Post",
  "draft": true
}
# Hello
"#;
        assert!(has_draft_frontmatter(content));
    }

    #[test]
    fn test_has_draft_frontmatter_with_json_false() {
        let content = r#"{
  "title": "Post",
  "draft": false
}
# Hello
"#;
        assert!(!has_draft_frontmatter(content));
    }

    #[test]
    fn test_parse_frontmatter_yaml_collects_standard_fields_and_extra() {
        let content = r#"---
title: "Post Title"
date: "2026-02-15"
draft: true
tags:
  - rust
  - web
categories: docs
custom_score: 42
author:
  name: James
---
# Hello
"#;

        let parsed = parse_frontmatter(content).expect("yaml frontmatter should parse");
        let (draft, title, date, tags, categories, extra) = parsed;

        assert_eq!(draft, Some(true));
        assert_eq!(title, Some("Post Title".to_string()));
        assert_eq!(date, Some("2026-02-15".to_string()));
        assert_eq!(tags, vec!["rust".to_string(), "web".to_string()]);
        assert_eq!(categories, vec!["docs".to_string()]);
        assert_eq!(
            extra.get("custom_score").and_then(|value| value.as_i64()),
            Some(42)
        );
        assert!(extra.contains_key("author"));
    }

    #[test]
    fn test_parse_frontmatter_json_collects_standard_fields_and_extra() {
        let content = r#"{
  "title": "Post Title",
  "date": "2026-02-15",
  "draft": "true",
  "tags": "rust, web",
  "categories": ["docs", "guides"],
  "custom_score": 42
}
# Hello
"#;

        let parsed = parse_frontmatter(content).expect("json frontmatter should parse");
        let (draft, title, date, tags, categories, extra) = parsed;

        assert_eq!(draft, Some(true));
        assert_eq!(title, Some("Post Title".to_string()));
        assert_eq!(date, Some("2026-02-15".to_string()));
        assert_eq!(tags, vec!["rust".to_string(), "web".to_string()]);
        assert_eq!(categories, vec!["docs".to_string(), "guides".to_string()]);
        assert_eq!(
            extra.get("custom_score").and_then(|value| value.as_i64()),
            Some(42)
        );
    }

    #[test]
    fn test_parse_frontmatter_without_frontmatter_returns_none() {
        let content = "# Just markdown\n\nNo frontmatter.";
        assert!(parse_frontmatter(content).is_none());
    }

    #[test]
    fn test_has_draft_frontmatter_with_unterminated_yaml() {
        let content = r#"---
title: Post
draft: true
# Hello
"#;
        assert!(!has_draft_frontmatter(content));
    }

    #[test]
    fn test_has_draft_frontmatter_with_unterminated_json() {
        let content = r#"{
  "title": "Post",
  "draft": true
# Hello
"#;
        assert!(!has_draft_frontmatter(content));
    }

    #[test]
    fn test_has_draft_frontmatter_with_invalid_json_syntax() {
        let content = r#"{
  "title": "Post",
  "draft": tru
}
# Hello
"#;
        assert!(!has_draft_frontmatter(content));
    }

    #[tokio::test]
    async fn test_save_endpoint_without_csrf_token() {
        let (app, temp_dir, _) = create_test_app().await;

        // Create a test markdown file
        let test_file = temp_dir.path().join("test.md");
        fs::write(&test_file, "# Test")
            .await
            .expect("Failed to write test file");

        // Try to save without CSRF token
        let request = Request::builder()
            .method(Method::POST)
            .uri("/save")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from("path=test.md&content=# Updated Content"))
            .expect("failed to build save request without csrf token");

        let response = app
            .oneshot(request)
            .await
            .expect("failed to send save request without csrf token");

        // Should return 422 Unprocessable Entity due to missing CSRF token
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_save_endpoint_with_invalid_csrf_token() {
        let (app, temp_dir, _) = create_test_app().await;

        // Create a test markdown file
        let test_file = temp_dir.path().join("test.md");
        fs::write(&test_file, "# Test")
            .await
            .expect("Failed to write test file");

        // Try to save with invalid CSRF token
        let request = Request::builder()
            .method(Method::POST)
            .uri("/save")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(
                "path=test.md&content=# Updated Content&csrf_token=invalid",
            ))
            .expect("failed to build save request with invalid csrf token");

        let response = app
            .oneshot(request)
            .await
            .expect("failed to send save request with invalid csrf token");

        // Should return 403 Forbidden due to invalid CSRF token
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let body = response
            .into_body()
            .collect()
            .await
            .expect("failed to collect response body for invalid csrf save")
            .to_bytes();
        let body_text = String::from_utf8(body.to_vec()).expect("Failed to get response body");
        assert!(
            body_text
                .to_ascii_lowercase()
                .contains("invalid csrf token")
        );

        // File should remain unchanged
        let content = fs::read_to_string(&test_file)
            .await
            .expect("failed to read file");
        assert_eq!(content, "# Test");
    }

    #[tokio::test]
    async fn test_save_endpoint_with_expired_csrf_token() {
        let (app, temp_dir, csrf_secret) = create_test_app().await;

        let test_file = temp_dir.path().join("test.md");
        fs::write(&test_file, "# Test")
            .await
            .expect("Failed to write test file");

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
            .expect("failed to build save request with expired csrf token");

        let response = app
            .oneshot(request)
            .await
            .expect("failed to send save request with expired csrf token");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let body = response
            .into_body()
            .collect()
            .await
            .expect("failed to collect response body for expired csrf save")
            .to_bytes();
        let body_text = String::from_utf8(body.to_vec()).expect("Failed to get response body");
        assert!(
            body_text
                .to_ascii_lowercase()
                .contains("invalid csrf token")
        );

        // File should remain unchanged
        let content = fs::read_to_string(&test_file)
            .await
            .expect("Failed to read test file");
        assert_eq!(content, "# Test");
    }

    #[tokio::test]
    async fn test_save_endpoint_with_valid_csrf_token() {
        let (app, temp_dir, csrf_secret) = create_test_app().await;

        // Create a test markdown file
        let test_file = temp_dir.path().join("test.md");
        fs::write(&test_file, "# Test")
            .await
            .expect("Failed to write test file");

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
            .expect("failed to build save request with valid csrf token");

        let response = app
            .oneshot(request)
            .await
            .expect("failed to send save request with valid csrf token");

        // Should succeed
        assert_eq!(response.status(), StatusCode::OK);

        // Verify file content was updated
        let content = fs::read_to_string(&test_file)
            .await
            .expect("Failed to read test file");
        assert_eq!(content, "# Updated Content");
    }

    #[tokio::test]
    async fn test_delete_endpoint_without_csrf_token() {
        let (app, temp_dir, _) = create_test_app().await;

        // Create a test markdown file
        let test_file = temp_dir.path().join("test.md");
        fs::write(&test_file, "# Test")
            .await
            .expect("Failed to write test file");

        // Try to delete without CSRF token
        let request = Request::builder()
            .method(Method::POST)
            .uri("/delete")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from("path=test.md"))
            .expect("failed to build delete request without csrf token");

        let response = app
            .oneshot(request)
            .await
            .expect("failed to send delete request without csrf token");

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
        fs::write(&test_file, "# Test")
            .await
            .expect("Failed to write test file");

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
            .expect("failed to build delete request with valid csrf token");

        let response = app
            .oneshot(request)
            .await
            .expect("failed to send delete request with valid csrf token");

        // Should succeed
        assert_eq!(response.status(), StatusCode::OK);

        // File should be deleted
        assert!(!test_file.exists());
    }

    #[tokio::test]
    async fn test_delete_endpoint_with_invalid_csrf_token() {
        let (app, temp_dir, _) = create_test_app().await;

        let test_file = temp_dir.path().join("test.md");
        fs::write(&test_file, "# Test")
            .await
            .expect("Failed to write test file");

        let request = Request::builder()
            .method(Method::POST)
            .uri("/delete")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from("path=test.md&csrf_token=invalid"))
            .expect("Failed to build request");

        let response = app.oneshot(request).await.expect("Failed to send request");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let body = response
            .into_body()
            .collect()
            .await
            .expect("Failed to collect response body")
            .to_bytes();
        let body_text = String::from_utf8(body.to_vec()).expect("Failed to get response body");
        assert!(
            body_text
                .to_ascii_lowercase()
                .contains("invalid csrf token")
        );

        // File should still exist
        assert!(test_file.exists());
    }

    #[tokio::test]
    async fn test_delete_endpoint_with_expired_csrf_token() {
        let (app, temp_dir, csrf_secret) = create_test_app().await;

        let test_file = temp_dir.path().join("test.md");
        fs::write(&test_file, "# Test")
            .await
            .expect("Failed to write test file");

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
            .expect("failed to build delete request with expired csrf token");

        let response = app
            .oneshot(request)
            .await
            .expect("failed to send delete request with expired csrf token");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let body = response
            .into_body()
            .collect()
            .await
            .expect("Failed to collect response body")
            .to_bytes();
        let body_text = String::from_utf8(body.to_vec()).expect("Failed to get response body");
        assert!(
            body_text
                .to_ascii_lowercase()
                .contains("invalid csrf token")
        );

        // File should still exist
        assert!(test_file.exists());
    }

    #[tokio::test]
    async fn test_edit_page_contains_csrf_token() {
        let (app, temp_dir, _) = create_test_app().await;

        // Create a test markdown file
        let test_file = temp_dir.path().join("test.md");
        fs::write(&test_file, "# Test Content")
            .await
            .expect("Failed to write test file");

        // Request the edit page
        let request = Request::builder()
            .method(Method::GET)
            .uri("/edit?path=test.md")
            .body(Body::empty())
            .expect("Failed to build request");

        let response = app.oneshot(request).await.expect("Failed to send request");
        assert_eq!(response.status(), StatusCode::OK);

        // Get response body
        let body = response
            .into_body()
            .collect()
            .await
            .expect("Failed to collect response body")
            .to_bytes();
        let html =
            String::from_utf8(body.to_vec()).expect("Failed to parse response body as UTF-8");

        // Verify CSRF token is present in both forms
        assert!(html.contains(r#"name="csrf_token""#));

        // Should have at least 2 CSRF token fields (save form and delete form)
        let csrf_count = html.matches(r#"name="csrf_token""#).count();
        assert_eq!(csrf_count, 2);
    }

    #[tokio::test]
    async fn test_edit_page_shows_draft_flag_for_yaml_frontmatter() {
        let (app, temp_dir, _) = create_test_app().await;
        let test_file = temp_dir.path().join("test.md");
        fs::write(
            &test_file,
            "---\ntitle: Test\ndraft: true\n---\n# Test Content",
        )
        .await
        .expect("Failed to write test file with yaml frontmatter");

        let request = Request::builder()
            .method(Method::GET)
            .uri("/edit?path=test.md")
            .body(Body::empty())
            .expect("Failed to build request");

        let response = app.oneshot(request).await.expect("Failed to send request");
        assert_eq!(response.status(), StatusCode::OK);

        let body = response
            .into_body()
            .collect()
            .await
            .expect("Failed to collect response body")
            .to_bytes();
        let html =
            String::from_utf8(body.to_vec()).expect("Failed to parse response body as UTF-8");
        assert!(html.contains(r#"class="draft-flag""#));
    }

    #[tokio::test]
    async fn test_edit_page_shows_draft_flag_for_json_frontmatter() {
        let (app, temp_dir, _) = create_test_app().await;
        let test_file = temp_dir.path().join("test.md");
        fs::write(
            &test_file,
            "{\n  \"title\": \"Test\",\n  \"draft\": true\n}\n# Test Content",
        )
        .await
        .expect("Failed to write test file with json frontmatter");

        let request = Request::builder()
            .method(Method::GET)
            .uri("/edit?path=test.md")
            .body(Body::empty())
            .expect("Failed to build request");

        let response = app.oneshot(request).await.expect("Failed to send request");
        assert_eq!(response.status(), StatusCode::OK);

        let body = response
            .into_body()
            .collect()
            .await
            .expect("Failed to collect response body")
            .to_bytes();
        let html =
            String::from_utf8(body.to_vec()).expect("Failed to parse response body as UTF-8");
        assert!(html.contains(r#"class="draft-flag""#));
    }

    #[tokio::test]
    async fn test_edit_page_hides_draft_flag_when_not_draft() {
        let (app, temp_dir, _) = create_test_app().await;
        let test_file = temp_dir.path().join("test.md");
        fs::write(
            &test_file,
            "---\ntitle: Test\ndraft: false\n---\n# Test Content",
        )
        .await
        .expect("Failed to write test file with non-draft frontmatter");

        let request = Request::builder()
            .method(Method::GET)
            .uri("/edit?path=test.md")
            .body(Body::empty())
            .expect("Failed to build request");

        let response = app.oneshot(request).await.expect("Failed to send request");
        assert_eq!(response.status(), StatusCode::OK);

        let body = response
            .into_body()
            .collect()
            .await
            .expect("Failed to collect response body")
            .to_bytes();
        let html =
            String::from_utf8(body.to_vec()).expect("Failed to parse response body as UTF-8");
        assert!(!html.contains(r#"class="draft-flag""#));
    }

    #[tokio::test]
    async fn test_index_page_contains_new_file_link() {
        let (app, _temp_dir, _) = create_test_app().await;

        let request = Request::builder()
            .method(Method::GET)
            .uri("/")
            .body(Body::empty())
            .expect("Failed to build index request");

        let response = app.oneshot(request).await.expect("Failed to send request");
        assert_eq!(response.status(), StatusCode::OK);

        let body = response
            .into_body()
            .collect()
            .await
            .expect("Failed to collect response body")
            .to_bytes();
        let html =
            String::from_utf8(body.to_vec()).expect("Failed to parse response body as UTF-8");
        assert!(html.contains(r#"href="/new-file""#));
    }

    #[tokio::test]
    async fn test_new_file_form_contains_path_and_csrf_token() {
        let (app, temp_dir, _) = create_test_app().await;

        let nested_dir = temp_dir.path().join("posts");
        fs::create_dir(&nested_dir)
            .await
            .expect("Failed to create nested directory");

        let request = Request::builder()
            .method(Method::GET)
            .uri("/new-file?path=posts")
            .body(Body::empty())
            .expect("Failed to build new-file request");

        let response = app.oneshot(request).await.expect("Failed to send request");
        assert_eq!(response.status(), StatusCode::OK);

        let body = response
            .into_body()
            .collect()
            .await
            .expect("Failed to collect response body")
            .to_bytes();
        let html =
            String::from_utf8(body.to_vec()).expect("Failed to parse response body as UTF-8");
        assert!(html.contains(r#"name="csrf_token""#));
        assert!(html.contains(r#"name="path" value="posts""#));
    }

    #[tokio::test]
    async fn test_create_new_file_redirects_to_editor() {
        let (app, temp_dir, csrf_secret) = create_test_app().await;
        let csrf_token = generate_csrf_token(&csrf_secret);

        let body = format!(
            "path=&filename=new-post&csrf_token={}",
            urlencoding::encode(&csrf_token)
        );
        let request = Request::builder()
            .method(Method::POST)
            .uri("/new-file")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .expect("Failed to build create-new-file request");

        let response = app.oneshot(request).await.expect("Failed to send request");
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response
                .headers()
                .get("location")
                .and_then(|h| h.to_str().ok()),
            Some("/edit?path=new-post.md")
        );

        let new_file = temp_dir.path().join("new-post.md");
        assert!(new_file.exists());
        let content = fs::read_to_string(&new_file)
            .await
            .expect("Failed to read created markdown file");
        assert!(content.is_empty());
    }

    #[tokio::test]
    async fn test_create_new_file_rejects_existing_file() {
        let (app, temp_dir, csrf_secret) = create_test_app().await;
        let existing_file = temp_dir.path().join("existing.md");
        fs::write(&existing_file, "# Existing")
            .await
            .expect("Failed to write existing markdown file");
        let csrf_token = generate_csrf_token(&csrf_secret);

        let body = format!(
            "path=&filename=existing&csrf_token={}",
            urlencoding::encode(&csrf_token)
        );
        let request = Request::builder()
            .method(Method::POST)
            .uri("/new-file")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .expect("Failed to build create-new-file request for existing file");

        let response = app.oneshot(request).await.expect("Failed to send request");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_create_new_file_rejects_invalid_filename() {
        let (app, _temp_dir, csrf_secret) = create_test_app().await;
        let csrf_token = generate_csrf_token(&csrf_secret);

        let body = format!(
            "path=&filename=bad%2Fname&csrf_token={}",
            urlencoding::encode(&csrf_token)
        );
        let request = Request::builder()
            .method(Method::POST)
            .uri("/new-file")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .expect("Failed to build create-new-file request with invalid filename");

        let response = app.oneshot(request).await.expect("Failed to send request");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_image_preview_page_contains_csrf_token() {
        let (app, temp_dir, _) = create_test_app().await;

        let test_file = temp_dir.path().join("image.png");
        fs::write(&test_file, "fake image data")
            .await
            .expect("Failed to write test file");

        let request = Request::builder()
            .method(Method::GET)
            .uri("/preview?path=image.png")
            .body(Body::empty())
            .expect("Failed to build request");

        let response = app.oneshot(request).await.expect("Failed to send request");
        assert_eq!(response.status(), StatusCode::OK);

        let body = response
            .into_body()
            .collect()
            .await
            .expect("Failed to collect response body")
            .to_bytes();
        let html =
            String::from_utf8(body.to_vec()).expect("Failed to parse response body as UTF-8");
        assert!(html.contains(r#"name="csrf_token""#));
    }

    #[tokio::test]
    async fn test_delete_from_image_preview_context_with_valid_csrf_token() {
        let (app, temp_dir, _) = create_test_app().await;

        let test_file = temp_dir.path().join("image.png");
        fs::write(&test_file, "fake image data")
            .await
            .expect("Failed to write test image file");

        let preview_request = Request::builder()
            .method(Method::GET)
            .uri("/preview?path=image.png")
            .body(Body::empty())
            .expect("Failed to build image preview request");
        let preview_response = app
            .clone()
            .oneshot(preview_request)
            .await
            .expect("Failed to send image preview request");
        assert_eq!(preview_response.status(), StatusCode::OK);

        let preview_body = preview_response
            .into_body()
            .collect()
            .await
            .expect("Failed to collect image preview body")
            .to_bytes();
        let preview_html = String::from_utf8(preview_body.to_vec())
            .expect("Failed to parse image preview body as UTF-8");
        let csrf_token = extract_csrf_token_from_html(&preview_html)
            .expect("Image preview HTML should contain csrf_token");

        let delete_body = format!(
            "path=image.png&csrf_token={}",
            urlencoding::encode(&csrf_token)
        );
        let delete_request = Request::builder()
            .method(Method::POST)
            .uri("/delete")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(delete_body))
            .expect("Failed to build delete request from image preview context");
        let delete_response = app
            .oneshot(delete_request)
            .await
            .expect("Failed to send delete request from image preview context");
        assert_eq!(delete_response.status(), StatusCode::OK);
        assert!(!test_file.exists());
    }

    #[tokio::test]
    async fn test_file_preview_page_contains_csrf_token() {
        let (app, temp_dir, _) = create_test_app().await;

        let test_file = temp_dir.path().join("notes.txt");
        fs::write(&test_file, "hello")
            .await
            .expect("Failed to write test file");

        let request = Request::builder()
            .method(Method::GET)
            .uri("/file-preview?path=notes.txt")
            .body(Body::empty())
            .expect("Failed to build request");

        let response = app.oneshot(request).await.expect("Failed to send request");
        assert_eq!(response.status(), StatusCode::OK);

        let body = response
            .into_body()
            .collect()
            .await
            .expect("Failed to collect response body")
            .to_bytes();
        let html =
            String::from_utf8(body.to_vec()).expect("Failed to parse response body as UTF-8");
        assert!(html.contains(r#"name="csrf_token""#));
    }

    #[tokio::test]
    async fn test_delete_from_file_preview_context_with_valid_csrf_token() {
        let (app, temp_dir, _) = create_test_app().await;

        let test_file = temp_dir.path().join("notes.txt");
        fs::write(&test_file, "hello")
            .await
            .expect("Failed to write test text file");

        let preview_request = Request::builder()
            .method(Method::GET)
            .uri("/file-preview?path=notes.txt")
            .body(Body::empty())
            .expect("Failed to build file preview request");
        let preview_response = app
            .clone()
            .oneshot(preview_request)
            .await
            .expect("Failed to send file preview request");
        assert_eq!(preview_response.status(), StatusCode::OK);

        let preview_body = preview_response
            .into_body()
            .collect()
            .await
            .expect("Failed to collect file preview body")
            .to_bytes();
        let preview_html =
            String::from_utf8(preview_body.to_vec()).expect("Failed to parse file preview HTML");
        let csrf_token = extract_csrf_token_from_html(&preview_html)
            .expect("File preview HTML should contain csrf_token");

        let delete_body = format!(
            "path=notes.txt&csrf_token={}",
            urlencoding::encode(&csrf_token)
        );
        let delete_request = Request::builder()
            .method(Method::POST)
            .uri("/delete")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(delete_body))
            .expect("Failed to build delete request from file preview context");
        let delete_response = app
            .oneshot(delete_request)
            .await
            .expect("Failed to send delete request from file preview context");
        assert_eq!(delete_response.status(), StatusCode::OK);
        assert!(!test_file.exists());
    }

    #[tokio::test]
    async fn test_file_info_endpoint_returns_json_metadata() {
        let (app, temp_dir, _) = create_test_app().await;
        let test_file = temp_dir.path().join("meta.md");
        fs::write(&test_file, "# Meta")
            .await
            .expect("Failed to write metadata test file");

        let request = Request::builder()
            .method(Method::GET)
            .uri("/file-info?path=meta.md")
            .body(Body::empty())
            .expect("Failed to build file-info request");

        let response = app
            .oneshot(request)
            .await
            .expect("Failed to send file-info request");
        assert_eq!(response.status(), StatusCode::OK);

        let body = response
            .into_body()
            .collect()
            .await
            .expect("Failed to collect file-info response body")
            .to_bytes();
        let json: serde_json::Value =
            serde_json::from_slice(&body).expect("Failed to decode file-info JSON");
        assert_eq!(
            json.get("size").and_then(serde_json::Value::as_u64),
            Some(6)
        );
        assert!(
            json.get("modified_time")
                .and_then(serde_json::Value::as_str)
                .is_some()
        );
    }

    #[tokio::test]
    async fn test_file_content_endpoint_returns_json_content() {
        let (app, temp_dir, _) = create_test_app().await;
        let test_file = temp_dir.path().join("content.md");
        fs::write(&test_file, "# Hello")
            .await
            .expect("Failed to write content test file");

        let request = Request::builder()
            .method(Method::GET)
            .uri("/file-content?path=content.md")
            .body(Body::empty())
            .expect("Failed to build file-content request");

        let response = app
            .oneshot(request)
            .await
            .expect("Failed to send file-content request");
        assert_eq!(response.status(), StatusCode::OK);

        let body = response
            .into_body()
            .collect()
            .await
            .expect("Failed to collect file-content response body")
            .to_bytes();
        let json: serde_json::Value =
            serde_json::from_slice(&body).expect("Failed to decode file-content JSON");
        assert_eq!(
            json.get("content").and_then(serde_json::Value::as_str),
            Some("# Hello")
        );
        assert!(
            json.get("modified_time")
                .and_then(serde_json::Value::as_str)
                .is_some()
        );
    }

    #[tokio::test]
    async fn test_file_content_endpoint_rejects_non_markdown() {
        let (app, temp_dir, _) = create_test_app().await;
        let test_file = temp_dir.path().join("notes.txt");
        fs::write(&test_file, "hello")
            .await
            .expect("Failed to write text test file");

        let request = Request::builder()
            .method(Method::GET)
            .uri("/file-content?path=notes.txt")
            .body(Body::empty())
            .expect("Failed to build file-content request for non-markdown");

        let response = app
            .oneshot(request)
            .await
            .expect("Failed to send file-content request for non-markdown");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_serve_image_sets_expected_content_type() {
        let (app, temp_dir, _) = create_test_app().await;
        let test_file = temp_dir.path().join("pixel.png");
        fs::write(&test_file, vec![0x89, b'P', b'N', b'G'])
            .await
            .expect("Failed to write png test file");

        let request = Request::builder()
            .method(Method::GET)
            .uri("/image?path=pixel.png")
            .body(Body::empty())
            .expect("Failed to build image request");

        let response = app
            .oneshot(request)
            .await
            .expect("Failed to send image request");
        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok());
        assert_eq!(content_type, Some("image/png"));
    }

    #[tokio::test]
    async fn test_serve_file_sets_security_headers_for_safe_file() {
        let (app, temp_dir, _) = create_test_app().await;
        let test_file = temp_dir.path().join("notes.txt");
        fs::write(&test_file, "safe preview")
            .await
            .expect("Failed to write safe file");

        let request = Request::builder()
            .method(Method::GET)
            .uri("/file?path=notes.txt")
            .body(Body::empty())
            .expect("Failed to build safe file request");

        let response = app
            .oneshot(request)
            .await
            .expect("Failed to send safe file request");
        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok());
        assert_eq!(content_type, Some("text/plain; charset=utf-8"));
        let nosniff = response
            .headers()
            .get("x-content-type-options")
            .and_then(|value| value.to_str().ok());
        assert_eq!(nosniff, Some("nosniff"));
        let frame_options = response
            .headers()
            .get("x-frame-options")
            .and_then(|value| value.to_str().ok());
        assert_eq!(frame_options, Some("SAMEORIGIN"));
    }

    #[tokio::test]
    async fn test_serve_file_forbids_unsafe_file_type() {
        let (app, temp_dir, _) = create_test_app().await;
        let test_file = temp_dir.path().join("script.sh");
        fs::write(&test_file, "echo hi")
            .await
            .expect("Failed to write executable test file");

        let request = Request::builder()
            .method(Method::GET)
            .uri("/file?path=script.sh")
            .body(Body::empty())
            .expect("Failed to build executable file request");

        let response = app
            .oneshot(request)
            .await
            .expect("Failed to send executable file request");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_file_preview_shows_iframe_for_safe_file_type() {
        let (app, temp_dir, _) = create_test_app().await;
        let test_file = temp_dir.path().join("notes.txt");
        fs::write(&test_file, "hello")
            .await
            .expect("Failed to write safe preview file");

        let request = Request::builder()
            .method(Method::GET)
            .uri("/file-preview?path=notes.txt")
            .body(Body::empty())
            .expect("Failed to build file preview request");

        let response = app
            .oneshot(request)
            .await
            .expect("Failed to send file preview request");
        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("Failed to collect file preview response")
            .to_bytes();
        let html =
            String::from_utf8(body.to_vec()).expect("Failed to parse file preview HTML as UTF-8");
        assert!(html.contains("<iframe "));
    }

    #[tokio::test]
    async fn test_file_preview_rejects_markdown_and_images() {
        let (app, temp_dir, _) = create_test_app().await;
        let markdown_file = temp_dir.path().join("post.md");
        let image_file = temp_dir.path().join("photo.png");
        fs::write(&markdown_file, "# Post")
            .await
            .expect("Failed to write markdown file");
        fs::write(&image_file, "not really png")
            .await
            .expect("Failed to write image file");

        let markdown_request = Request::builder()
            .method(Method::GET)
            .uri("/file-preview?path=post.md")
            .body(Body::empty())
            .expect("Failed to build markdown file-preview request");
        let markdown_response = app
            .clone()
            .oneshot(markdown_request)
            .await
            .expect("Failed to send markdown file-preview request");
        assert_eq!(markdown_response.status(), StatusCode::BAD_REQUEST);

        let image_request = Request::builder()
            .method(Method::GET)
            .uri("/file-preview?path=photo.png")
            .body(Body::empty())
            .expect("Failed to build image file-preview request");
        let image_response = app
            .oneshot(image_request)
            .await
            .expect("Failed to send image file-preview request");
        assert_eq!(image_response.status(), StatusCode::BAD_REQUEST);
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
        assert!(validate_csrf_token(&token1, secret).is_ok());
        assert!(validate_csrf_token(&token2, secret).is_ok());
        assert!(validate_csrf_token(&token3, secret).is_ok());
    }

    #[test]
    fn test_git_compatible_ascii_filename_stem_rules() {
        assert!(is_git_compatible_ascii_filename_stem("post"));
        assert!(is_git_compatible_ascii_filename_stem("post-1_2.test"));
        assert!(!is_git_compatible_ascii_filename_stem(""));
        assert!(!is_git_compatible_ascii_filename_stem(".post"));
        assert!(!is_git_compatible_ascii_filename_stem("post."));
        assert!(!is_git_compatible_ascii_filename_stem("post..v1"));
        assert!(!is_git_compatible_ascii_filename_stem("bad/name"));
        assert!(!is_git_compatible_ascii_filename_stem("two words"));
        assert!(!is_git_compatible_ascii_filename_stem("ümlaut"));
    }

    #[test]
    fn test_normalize_markdown_filename_handles_common_inputs() {
        assert_eq!(
            normalize_markdown_filename("post").ok(),
            Some("post.md".to_string())
        );
        assert_eq!(
            normalize_markdown_filename("post.md").ok(),
            Some("post.md".to_string())
        );
        assert_eq!(
            normalize_markdown_filename("post.markdown").ok(),
            Some("post.md".to_string())
        );
        assert_eq!(
            normalize_markdown_filename("  post-with-space-trim  ").ok(),
            Some("post-with-space-trim.md".to_string())
        );
        assert!(normalize_markdown_filename("").is_err());
        assert!(normalize_markdown_filename("bad/name").is_err());
        assert!(normalize_markdown_filename(".hidden").is_err());
        assert!(normalize_markdown_filename("bad..name").is_err());
    }

    #[test]
    fn test_extract_frontmatter_detects_yaml_and_json_blocks() {
        let yaml_content = "---\ndraft: true\n---\n# Post\n";
        let json_content = "{\n  \"draft\": true\n}\n# Post\n";
        let no_frontmatter = "# Post";

        let yaml = extract_frontmatter(yaml_content).expect("yaml frontmatter should be found");
        assert!(matches!(yaml.0, FrontmatterFormat::Yaml));
        assert_eq!(yaml.1, "draft: true\n");

        let json = extract_frontmatter(json_content).expect("json frontmatter should be found");
        assert!(matches!(json.0, FrontmatterFormat::Json));
        assert!(json.1.contains("\"draft\": true"));

        assert!(extract_frontmatter(no_frontmatter).is_none());
    }

    #[test]
    fn test_extract_json_frontmatter_requires_newline_or_eof_after_block() {
        let valid_with_newline = "{\n  \"draft\": true\n}\n# post";
        let valid_eof = "{\n  \"draft\": true\n}";
        let invalid_trailing_text = "{\"draft\":true}#post";

        assert!(extract_json_frontmatter(valid_with_newline).is_some());
        assert!(extract_json_frontmatter(valid_eof).is_some());
        assert!(extract_json_frontmatter(invalid_trailing_text).is_none());
    }

    #[test]
    fn test_parse_bool_value_coercions() {
        assert_eq!(parse_bool_value(&serde_json::json!(true)), Some(true));
        assert_eq!(parse_bool_value(&serde_json::json!(false)), Some(false));
        assert_eq!(parse_bool_value(&serde_json::json!("yes")), Some(true));
        assert_eq!(parse_bool_value(&serde_json::json!("off")), Some(false));
        assert_eq!(parse_bool_value(&serde_json::json!(1)), Some(true));
        assert_eq!(parse_bool_value(&serde_json::json!(0)), Some(false));
        assert_eq!(parse_bool_value(&serde_json::json!(2)), None);
        assert_eq!(parse_bool_value(&serde_json::json!("maybe")), None);
    }

    #[test]
    fn test_parse_string_value_coercions() {
        assert_eq!(
            parse_string_value(&serde_json::json!(" hello ")),
            Some("hello".to_string())
        );
        assert_eq!(
            parse_string_value(&serde_json::json!(42)),
            Some("42".to_string())
        );
        assert_eq!(
            parse_string_value(&serde_json::json!(true)),
            Some("true".to_string())
        );
        assert_eq!(parse_string_value(&serde_json::json!("  ")), None);
        assert_eq!(parse_string_value(&serde_json::json!({"k": "v"})), None);
    }

    #[test]
    fn test_parse_string_list_value_coercions() {
        assert_eq!(
            parse_string_list_value(&serde_json::json!(["a", " b ", 3])),
            vec!["a".to_string(), "b".to_string(), "3".to_string()]
        );
        assert_eq!(
            parse_string_list_value(&serde_json::json!("rust, web,  docs ")),
            vec!["rust".to_string(), "web".to_string(), "docs".to_string()]
        );
        assert_eq!(
            parse_string_list_value(&serde_json::json!("single")),
            vec!["single".to_string()]
        );
        assert_eq!(
            parse_string_list_value(&serde_json::json!(9)),
            vec!["9".to_string()]
        );
    }

    #[test]
    fn test_format_file_size_units() {
        assert_eq!(format_file_size(0), "0 B");
        assert_eq!(format_file_size(1023), "1023 B");
        assert_eq!(format_file_size(1024), "1.0 KB");
        assert_eq!(format_file_size(1_048_576), "1.0 MB");
    }

    #[test]
    fn test_get_parent_directory_path_formats_navigation_urls() {
        assert_eq!(get_parent_directory_path("post.md"), "/");
        assert_eq!(get_parent_directory_path("posts/post.md"), "/?path=posts");
        assert_eq!(
            get_parent_directory_path("posts/2026/post.md"),
            "/?path=posts%2F2026"
        );
    }

    #[test]
    fn test_get_file_type_description_covers_known_and_unknown_types() {
        assert_eq!(get_file_type_description("notes.txt"), "Text file");
        assert_eq!(get_file_type_description("index.html"), "HTML document");
        assert_eq!(get_file_type_description("data.json"), "JSON data");
        assert_eq!(get_file_type_description("script.sh"), "Executable file");
        assert_eq!(
            get_file_type_description("archive.unknown"),
            "Unknown file type"
        );
    }

    #[test]
    fn test_build_breadcrumbs_generates_expected_paths() {
        let breadcrumbs = build_breadcrumbs("posts/2026");
        assert_eq!(breadcrumbs.len(), 2);

        let first = breadcrumbs
            .first()
            .expect("breadcrumbs should have a first element");
        assert_eq!(first.name, "posts");
        assert_eq!(first.url, "/?path=posts");

        let second = breadcrumbs
            .get(1)
            .expect("breadcrumbs should have a second element");
        assert_eq!(second.name, "2026");
        assert_eq!(second.url, "/?path=posts%2F2026");
    }

    #[test]
    fn test_build_directory_entry_views_maps_file_types_to_view_models() {
        let entries = vec![
            DirectoryEntry {
                name: "posts".to_string(),
                is_directory: true,
                path: "posts".to_string(),
            },
            DirectoryEntry {
                name: "note.md".to_string(),
                is_directory: false,
                path: "posts/note.md".to_string(),
            },
            DirectoryEntry {
                name: "photo.png".to_string(),
                is_directory: false,
                path: "posts/photo.png".to_string(),
            },
            DirectoryEntry {
                name: "run.sh".to_string(),
                is_directory: false,
                path: "posts/run.sh".to_string(),
            },
            DirectoryEntry {
                name: "notes.txt".to_string(),
                is_directory: false,
                path: "posts/notes.txt".to_string(),
            },
        ];

        let views = build_directory_entry_views(&entries);
        assert_eq!(views.len(), 5);

        let directory_view = views.first().expect("expected directory view");
        assert_eq!(directory_view.icon, "📁");
        assert!(directory_view.has_url);

        let markdown_view = views.get(1).expect("expected markdown view");
        assert!(markdown_view.url.starts_with("/edit?path="));
        assert!(markdown_view.has_url);

        let image_view = views.get(2).expect("expected image view");
        assert!(image_view.url.starts_with("/preview?path="));
        assert!(image_view.has_url);

        let executable_view = views.get(3).expect("expected executable view");
        assert!(!executable_view.has_url);
        assert!(executable_view.executable);

        let generic_file_view = views.get(4).expect("expected generic file view");
        assert!(generic_file_view.url.starts_with("/file-preview?path="));
        assert!(generic_file_view.has_url);
    }

    #[tokio::test]
    async fn test_templates_avoid_inline_js_and_css() {
        let template_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates");
        let mut entries = fs::read_dir(&template_dir)
            .await
            .expect("failed to read templates directory");

        let inline_event_attributes = [
            "onclick=",
            "onload=",
            "onerror=",
            "onsubmit=",
            "onchange=",
            "oninput=",
            "onfocus=",
            "onblur=",
            "onkeydown=",
            "onkeyup=",
            "onkeypress=",
            "onmouseover=",
            "onmouseout=",
        ];

        while let Some(entry) = entries
            .next_entry()
            .await
            .expect("failed to read templates directory entry")
        {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("html") {
                continue;
            }

            let content = fs::read_to_string(&path)
                .await
                .expect("failed to read template for inline js/css checks");
            let lower = content.to_ascii_lowercase();

            assert!(
                !lower.contains("<style"),
                "template contains inline <style> tag: {}",
                path.display()
            );
            assert!(
                !lower.contains("style=\""),
                "template contains inline style attribute: {}",
                path.display()
            );

            for attr in inline_event_attributes {
                assert!(
                    !lower.contains(attr),
                    "template contains inline event handler '{}': {}",
                    attr,
                    path.display()
                );
            }

            let mut search_from = 0;
            while let Some(rel_index) = lower[search_from..].find("<script") {
                let start = search_from + rel_index;
                let tag_end = lower[start..]
                    .find('>')
                    .expect("script tag should have a closing angle bracket");
                let tag = &lower[start..start + tag_end + 1];
                assert!(
                    tag.contains("src="),
                    "template contains inline <script> tag without src: {}",
                    path.display()
                );
                search_from = start + tag_end + 1;
            }
        }
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
