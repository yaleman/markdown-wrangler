use axum::{
    Router,
    extract::{Form, Query, State},
    http::StatusCode,
    response::{Html, Json},
    routing::{get, post},
};
use axum_tracing_opentelemetry::middleware::{OtelAxumLayer, OtelInResponseLayer};
use rand::Rng;
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
    let nonce: u64 = rand::thread_rng().r#gen();

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
    "txt", "html", "htm", "css", "js", "json", "xml", "pdf", "csv", "log", "yml", "yaml", "toml", "ini", "conf", "cfg",
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

fn generate_editor_html(file_path: &str, content: &str, csrf_token: &str) -> String {
    let escaped_content = html_escape::encode_text(content);
    let escaped_path = html_escape::encode_text(file_path);
    let escaped_csrf_token = html_escape::encode_text(csrf_token);

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>Markdown Wrangler - Edit {file_path}</title>
    <link rel="stylesheet" href="/static/styles.css">
</head>
<body>
    <h1>📝 Edit Markdown File</h1>
    <div class="breadcrumb">
        <a href="/">← Back to file browser</a> | 📄 {escaped_path}
    </div>
    
    <form method="post" action="/save">
        <input type="hidden" name="path" value="{escaped_path}" />
        <input type="hidden" name="csrf_token" value="{escaped_csrf_token}" />
        <div class="buttons">
            <button type="submit">💾 Save File</button>
            <button type="button" class="cancel" onclick="window.location.href='/'">❌ Cancel</button>
            <button type="button" class="delete-btn" onclick="confirmDelete('{escaped_path}')">🗑️ Delete File</button>
        </div>
        
        <div class="editor-container">
            <div class="editor-panel">
                <h3>📝 Editor</h3>
                <textarea name="content" placeholder="Enter your markdown content here...">{escaped_content}</textarea>
            </div>
            <div class="editor-panel">
                <h3>👁️ Preview</h3>
                <div class="preview" id="preview">
                    <p><em>Preview will appear here as you type...</em></p>
                </div>
            </div>
        </div>
    </form>
    
    <form id="deleteForm" method="post" action="/delete" style="display: none;">
        <input type="hidden" name="path" value="{escaped_path}" />
        <input type="hidden" name="csrf_token" value="{escaped_csrf_token}" />
    </form>

    <script src="/static/editor.js"></script>
    <script src="/static/editor-storage.js"></script>
    <script src="/static/delete.js"></script>
</body>
</html>"#
    )
}

fn generate_image_preview_html(file_path: &str, file_size: &str) -> String {
    let escaped_path = html_escape::encode_text(file_path);
    let parent_path = get_parent_directory_path(file_path);

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>Markdown Wrangler - Image Preview: {file_path}</title>
    <link rel="stylesheet" href="/static/styles.css">
</head>
<body>
    <h1>🖼️ Image Preview</h1>
    <div class="breadcrumb">
        <a href="/">← Back to file browser</a> | 🖼️ {escaped_path}
    </div>
    
    <div class="image-preview-container">
        <div class="image-wrapper">
            <img src="/image?path={encoded_path}" alt="{escaped_path}" class="preview-image" id="previewImage" onload="updateImageDimensions()" />
        </div>
        <div class="image-info">
            <h3>📄 File Information</h3>
            <p><strong>File:</strong> {escaped_path}</p>
            <p><strong>Size:</strong> {file_size}</p>
            <p><strong>Dimensions:</strong> <span id="imageDimensions">Loading...</span></p>
        </div>
    </div>
    
    <div class="buttons">
        <button onclick="window.location.href='{parent_path}'">📁 Back to Files</button>
        <button class="delete-btn" onclick="confirmDelete('{escaped_path}')">🗑️ Delete File</button>
    </div>
    
    <form id="deleteForm" method="post" action="/delete" style="display: none;">
        <input type="hidden" name="path" value="{escaped_path}" />
    </form>
    
    <script>
        function updateImageDimensions() {{
            const img = document.getElementById('previewImage');
            const dimensionsSpan = document.getElementById('imageDimensions');
            if (img && dimensionsSpan) {{
                dimensionsSpan.textContent = `${{img.naturalWidth}} × ${{img.naturalHeight}} pixels`;
            }}
        }}
    </script>
    <script src="/static/delete.js"></script>
</body>
</html>"#,
        file_path = file_path,
        escaped_path = escaped_path,
        encoded_path = urlencoding::encode(file_path),
        file_size = file_size,
        parent_path = parent_path
    )
}

fn generate_file_preview_html(file_path: &str, file_size: &str) -> String {
    let escaped_path = html_escape::encode_text(file_path);
    let parent_path = get_parent_directory_path(file_path);
    let can_iframe = is_safe_for_iframe(file_path);

    let preview_content = if can_iframe {
        format!(
            r#"<div class="file-preview-iframe">
                <iframe src="/file?path={}" frameborder="0" sandbox="allow-same-origin"></iframe>
               </div>"#,
            urlencoding::encode(file_path)
        )
    } else {
        r#"<div class="file-preview-message">
            <p>⚠️ File preview not available for this file type for security reasons.</p>
            <p>This file type cannot be safely displayed in the browser.</p>
           </div>"#
            .to_string()
    };

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>Markdown Wrangler - File Preview: {file_path}</title>
    <link rel="stylesheet" href="/static/styles.css">
</head>
<body>
    <h1>📄 File Preview</h1>
    <div class="breadcrumb">
        <a href="/">← Back to file browser</a> | 📄 {escaped_path}
    </div>
    
    <div class="file-preview-container">
        {preview_content}
        <div class="file-info">
            <h3>📄 File Information</h3>
            <p><strong>File:</strong> {escaped_path}</p>
            <p><strong>Size:</strong> {file_size}</p>
            <p><strong>Type:</strong> {file_type}</p>
        </div>
    </div>
    
    <div class="buttons">
        <button onclick="window.location.href='{parent_path}'">📁 Back to Files</button>
        <button class="delete-btn" onclick="confirmDelete('{escaped_path}')">🗑️ Delete File</button>
    </div>
    
    <form id="deleteForm" method="post" action="/delete" style="display: none;">
        <input type="hidden" name="path" value="{escaped_path}" />
    </form>
    
    <script src="/static/delete.js"></script>
</body>
</html>"#,
        file_path = file_path,
        escaped_path = escaped_path,
        file_size = file_size,
        file_type = get_file_type_description(file_path),
        preview_content = preview_content,
        parent_path = parent_path
    )
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

fn generate_directory_html(entries: &[DirectoryEntry], current_path: &str) -> String {
    let mut html = String::from(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>Markdown Wrangler - Directory Browser</title>
    <link rel="stylesheet" href="/static/styles.css">
</head>
<body style="margin: 40px;">
    <h1>📁 Markdown Wrangler</h1>
"#,
    );

    // Add breadcrumb navigation
    html.push_str("<div class=\"breadcrumb\">");
    html.push_str("📍 Path: ");
    if current_path.is_empty() {
        html.push_str("<strong>/</strong>");
    } else {
        html.push_str("<a href=\"/\">root</a>");
        let mut path_so_far = String::new();
        for part in current_path.split('/') {
            if !path_so_far.is_empty() {
                path_so_far.push('/');
            }
            path_so_far.push_str(part);
            html.push_str(&format!(
                " / <a href=\"/?path={}\">{part}</a>",
                urlencoding::encode(&path_so_far)
            ));
        }
    }
    html.push_str("</div>");

    // Add parent directory link if not at root
    if !current_path.is_empty() {
        let parent_path = if let Some(pos) = current_path.rfind('/') {
            &current_path[..pos]
        } else {
            ""
        };
        let parent_url = if parent_path.is_empty() {
            "/".to_string()
        } else {
            format!("/?path={}", urlencoding::encode(parent_path))
        };
        html.push_str(&format!(
            "<div class=\"entry\"><a href=\"{parent_url}\">📁 <span class=\"directory\">..</span></a></div>"
        ));
    }

    // Add directory entries
    for entry in entries {
        let icon = if entry.is_directory { "📁" } else { "📄" };
        let class = if entry.is_directory {
            "directory"
        } else {
            "file"
        };

        if entry.is_directory {
            let encoded_path = urlencoding::encode(&entry.path);
            html.push_str(&format!(
                "<div class=\"entry\"><a href=\"/?path={}\"><span class=\"icon\">{}</span><span class=\"{}\">{}</span></a></div>",
                encoded_path, icon, class, entry.name
            ));
        } else if is_markdown_file(&entry.name) {
            let encoded_path = urlencoding::encode(&entry.path);
            html.push_str(&format!(
                "<div class=\"entry\"><a href=\"/edit?path={}\"><span class=\"icon\">{}</span><span class=\"{}\">{}</span></a></div>",
                encoded_path, icon, class, entry.name
            ));
        } else if is_image_file(&entry.name) {
            let encoded_path = urlencoding::encode(&entry.path);
            html.push_str(&format!(
                "<div class=\"entry\"><a href=\"/preview?path={}\"><span class=\"icon\">🖼️</span><span class=\"{}\">{}</span></a></div>",
                encoded_path, class, entry.name
            ));
        } else if is_executable_file(&entry.name) {
            // Don't make executable files clickable for security
            html.push_str(&format!(
                "<div class=\"entry\"><span class=\"icon\">⚠️</span><span class=\"{} executable\">{}</span> <small>(executable)</small></div>",
                class, entry.name
            ));
        } else {
            // Generic file preview for other file types
            let encoded_path = urlencoding::encode(&entry.path);
            html.push_str(&format!(
                "<div class=\"entry\"><a href=\"/file-preview?path={}\"><span class=\"icon\">{}</span><span class=\"{}\">{}</span></a></div>",
                encoded_path, icon, class, entry.name
            ));
        }
    }

    html.push_str("</body></html>");
    html
}

async fn index(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<Html<String>, (StatusCode, String)> {
    let path = params.get("path").map(|s| s.as_str()).unwrap_or("");

    match list_directory(&state.target_dir, path) {
        Ok(entries) => {
            let html = generate_directory_html(&entries, path);
            Ok(Html(html))
        }
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
                let html = generate_editor_html(file_path, &content, &csrf_token);
                Ok(Html(html))
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
                        let no_change_html = format!(
                            r#"<!DOCTYPE html>
<html>
<head>
    <title>File Unchanged - Markdown Wrangler</title>
    <link rel="stylesheet" href="/static/styles.css">
</head>
<body class="center">
    <h1 class="success">ℹ️ No Changes to Save</h1>
    <p>The file <strong>{}</strong> content is unchanged.</p>
    <div class="buttons">
        <button class="save-buttons" onclick="window.location.href='/edit?path={}'">📝 Continue Editing</button>
        <button class="save-buttons" onclick="window.location.href='{}'">📁 Back to Files</button>
    </div>
</body>
</html>"#,
                            html_escape::encode_text(&form.path),
                            urlencoding::encode(&form.path),
                            parent_path
                        );
                        Ok(Html(no_change_html))
                    } else {
                        // Content has changed, write to disk
                        match write_file_content(&full_path, &form.content) {
                            Ok(()) => {
                                info!("File saved successfully: {}", form.path);
                                let parent_path = get_parent_directory_path(&form.path);
                                let success_html = format!(
                                    r#"<!DOCTYPE html>
<html>
<head>
    <title>File Saved - Markdown Wrangler</title>
    <link rel="stylesheet" href="/static/styles.css">
</head>
<body class="center">
    <h1 class="success">✅ File Saved Successfully!</h1>
                                    <p>The file <strong>{}</strong> has been saved.</p>
    <div class="buttons">
        <button class="save-buttons" onclick="window.location.href='/edit?path={}'">📝 Continue Editing</button>
        <button class="save-buttons" onclick="window.location.href='{}'">📁 Back to Files</button>
    </div>
</body>
</html>"#,
                                    html_escape::encode_text(&form.path),
                                    urlencoding::encode(&form.path),
                                    parent_path
                                );
                                Ok(Html(success_html))
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
            match get_file_size(&full_path) {
                Ok(size_bytes) => {
                    let file_size = format_file_size(size_bytes);
                    let html = generate_image_preview_html(file_path, &file_size);
                    Ok(Html(html))
                }
                Err(err) => {
                    warn!("Failed to get file size: {}", err);
                    // Fall back to generating without size info
                    let html = generate_image_preview_html(file_path, "Unknown");
                    Ok(Html(html))
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
            match get_file_size(&full_path) {
                Ok(size_bytes) => {
                    let file_size = format_file_size(size_bytes);
                    let html = generate_file_preview_html(file_path, &file_size);
                    Ok(Html(html))
                }
                Err(err) => {
                    warn!("Failed to get file size: {}", err);
                    // Fall back to generating without size info
                    let html = generate_file_preview_html(file_path, "Unknown");
                    Ok(Html(html))
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
                let success_html = format!(
                    r#"<!DOCTYPE html>
<html>
<head>
    <title>File Deleted - Markdown Wrangler</title>
    <link rel="stylesheet" href="/static/styles.css">
</head>
<body class="center">
    <h1 class="success">🗑️ File Deleted Successfully!</h1>
    <p>The file <strong>{}</strong> has been deleted.</p>
    <div class="buttons">
        <button class="save-buttons" onclick="window.location.href='{}'">📁 Back to Files</button>
    </div>
</body>
</html>"#,
                    html_escape::encode_text(&form.path),
                    parent_path
                );
                Ok(Html(success_html))
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
    let csrf_secret = hex::encode(rand::thread_rng().r#gen::<[u8; 32]>());
    let state = AppState {
        target_dir,
        csrf_secret,
    };
    #[allow(clippy::default_constructed_unit_structs)]
    let app = create_router(state)
        .layer(OtelInResponseLayer::default())
        .layer(OtelAxumLayer::default());

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
