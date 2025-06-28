use axum::{
    Router,
    extract::{Form, Query, State},
    http::StatusCode,
    response::Html,
    routing::{get, post},
};
use axum_tracing_opentelemetry::middleware::{OtelAxumLayer, OtelInResponseLayer};
use serde::Deserialize;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};
use tokio::net::TcpListener;
use tower_http::services::ServeDir;
use tracing::{info, warn};

#[derive(Clone)]
pub struct AppState {
    pub target_dir: PathBuf,
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
        .map_err(|e| format!("Base directory error: {}", e))?;
    let canonical_full = full_path
        .canonicalize()
        .map_err(|e| format!("Path error: {}", e))?;

    if !canonical_full.starts_with(&canonical_base) {
        return Err("Path outside base directory".to_string());
    }

    let entries =
        fs::read_dir(&full_path).map_err(|e| format!("Failed to read directory: {}", e))?;

    let mut directory_entries = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let file_name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden files
        if file_name.starts_with('.') {
            continue;
        }

        let is_directory = entry
            .file_type()
            .map_err(|e| format!("Failed to get file type: {}", e))?
            .is_dir();

        let entry_path = if relative_path.is_empty() {
            file_name.clone()
        } else {
            format!("{}/{}", relative_path, file_name)
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
        .map_err(|e| format!("Base directory error: {}", e))?;
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

fn is_image_file(path: &str) -> bool {
    let lower_path = path.to_lowercase();
    lower_path.ends_with(".jpg") 
        || lower_path.ends_with(".jpeg") 
        || lower_path.ends_with(".png") 
        || lower_path.ends_with(".gif") 
        || lower_path.ends_with(".webp") 
        || lower_path.ends_with(".svg") 
        || lower_path.ends_with(".bmp") 
        || lower_path.ends_with(".tiff") 
        || lower_path.ends_with(".tif")
}

fn read_file_content(file_path: &Path) -> Result<String, String> {
    fs::read_to_string(file_path).map_err(|e| format!("Failed to read file: {}", e))
}

fn write_file_content(file_path: &Path, content: &str) -> Result<(), String> {
    fs::write(file_path, content).map_err(|e| format!("Failed to write file: {}", e))
}

fn generate_editor_html(file_path: &str, content: &str) -> String {
    let escaped_content = html_escape::encode_text(content);
    let escaped_path = html_escape::encode_text(file_path);

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
        <div class="buttons">
            <button type="submit">💾 Save File</button>
            <button type="button" class="cancel" onclick="window.location.href='/'">❌ Cancel</button>
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

    <script src="/static/editor.js"></script>
</body>
</html>"#,
        file_path = file_path,
        escaped_path = escaped_path,
        escaped_content = escaped_content
    )
}

fn generate_image_preview_html(file_path: &str) -> String {
    let escaped_path = html_escape::encode_text(file_path);
    
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
            <img src="/image?path={encoded_path}" alt="{escaped_path}" class="preview-image" />
        </div>
        <div class="image-info">
            <h3>📄 File Information</h3>
            <p><strong>File:</strong> {escaped_path}</p>
        </div>
    </div>
    
    <div class="buttons">
        <button onclick="window.location.href='/'">📁 Back to Files</button>
    </div>
</body>
</html>"#,
        file_path = file_path,
        escaped_path = escaped_path,
        encoded_path = urlencoding::encode(file_path)
    )
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
                " / <a href=\"/?path={}\">{}</a>",
                urlencoding::encode(&path_so_far),
                part
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
            "<div class=\"entry\"><a href=\"{}\">📁 <span class=\"directory\">..</span></a></div>",
            parent_url
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
        } else {
            html.push_str(&format!(
                "<div class=\"entry\"><span class=\"icon\">{}</span><span class=\"{}\">{}</span></div>",
                icon, class, entry.name
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
            Err((StatusCode::BAD_REQUEST, format!("Error: {}", err)))
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
                let html = generate_editor_html(file_path, &content);
                Ok(Html(html))
            }
            Err(err) => {
                warn!("File read error: {}", err);
                Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Error reading file: {}", err),
                ))
            }
        },
        Err(err) => {
            warn!("File validation error: {}", err);
            Err((StatusCode::BAD_REQUEST, format!("Error: {}", err)))
        }
    }
}

async fn save_file(
    State(state): State<AppState>,
    Form(form): Form<EditForm>,
) -> Result<Html<String>, (StatusCode, String)> {
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
        <button class="save-buttons" onclick="window.location.href='/'">📁 Back to Files</button>
    </div>
</body>
</html>"#,
                            html_escape::encode_text(&form.path),
                            urlencoding::encode(&form.path)
                        );
                        Ok(Html(no_change_html))
                    } else {
                        // Content has changed, write to disk
                        match write_file_content(&full_path, &form.content) {
                            Ok(()) => {
                                info!("File saved successfully: {}", form.path);
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
        <button class="save-buttons" onclick="window.location.href='/'">📁 Back to Files</button>
    </div>
</body>
</html>"#,
                                    html_escape::encode_text(&form.path),
                                    urlencoding::encode(&form.path)
                                );
                                Ok(Html(success_html))
                            }
                            Err(err) => {
                                warn!("File save error: {}", err);
                                Err((
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    format!("Error saving file: {}", err),
                                ))
                            }
                        }
                    }
                }
                Err(err) => {
                    warn!("File read error during save comparison: {}", err);
                    Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Error reading file for comparison: {}", err),
                    ))
                }
            }
        }
        Err(err) => {
            warn!("File validation error during save: {}", err);
            Err((StatusCode::BAD_REQUEST, format!("Error: {}", err)))
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
        Ok(_) => {
            let html = generate_image_preview_html(file_path);
            Ok(Html(html))
        }
        Err(err) => {
            warn!("File validation error: {}", err);
            Err((StatusCode::BAD_REQUEST, format!("Error: {}", err)))
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
                        format!("Error reading image: {}", err),
                    ))
                }
            }
        }
        Err(err) => {
            warn!("Image validation error: {}", err);
            Err((StatusCode::BAD_REQUEST, format!("Error: {}", err)))
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
        .route("/preview", get(preview_image))
        .route("/image", get(serve_image))
        .nest_service("/static", ServeDir::new("static"))
        .fallback(handler_404)
        .with_state(state)
}

pub async fn start_server(target_dir: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let state = AppState { target_dir };
    #[allow(clippy::default_constructed_unit_structs)]
    let app = create_router(state)
        .layer(OtelInResponseLayer::default())
        .layer(OtelAxumLayer::default());

    let listener = TcpListener::bind("0.0.0.0:5420").await?;
    info!("Web server listening on http://0.0.0.0:5420, press Ctrl+C to stop");

    axum::serve(listener, app).await?;

    Ok(())
}
