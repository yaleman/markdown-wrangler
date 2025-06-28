use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Html,
    routing::get,
    Router,
};
use axum_tracing_opentelemetry::middleware::{OtelAxumLayer, OtelInResponseLayer};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};
use tokio::net::TcpListener;
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

fn list_directory(base_dir: &Path, relative_path: &str) -> Result<Vec<DirectoryEntry>, String> {
    let full_path = if relative_path.is_empty() {
        base_dir.to_path_buf()
    } else {
        base_dir.join(relative_path)
    };

    // Security check: ensure the path is within the base directory
    let canonical_base = base_dir.canonicalize().map_err(|e| format!("Base directory error: {}", e))?;
    let canonical_full = full_path.canonicalize().map_err(|e| format!("Path error: {}", e))?;
    
    if !canonical_full.starts_with(&canonical_base) {
        return Err("Path outside base directory".to_string());
    }

    let entries = fs::read_dir(&full_path)
        .map_err(|e| format!("Failed to read directory: {}", e))?;

    let mut directory_entries = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let file_name = entry.file_name().to_string_lossy().to_string();
        
        // Skip hidden files
        if file_name.starts_with('.') {
            continue;
        }

        let is_directory = entry.file_type()
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
    directory_entries.sort_by(|a, b| {
        match (a.is_directory, b.is_directory) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        }
    });

    Ok(directory_entries)
}

fn generate_directory_html(entries: &[DirectoryEntry], current_path: &str) -> String {
    let mut html = String::from(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>Markdown Wrangler - Directory Browser</title>
    <style>
        body { font-family: Arial, sans-serif; margin: 40px; }
        h1 { color: #333; }
        .breadcrumb { margin-bottom: 20px; color: #666; }
        .entry { margin: 5px 0; }
        .directory { font-weight: bold; }
        .file { color: #666; }
        a { text-decoration: none; color: #0066cc; }
        a:hover { text-decoration: underline; }
        .icon { margin-right: 8px; }
    </style>
</head>
<body>
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
            html.push_str(&format!(" / <a href=\"/?path={}\">{}</a>", 
                urlencoding::encode(&path_so_far), part));
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
        let class = if entry.is_directory { "directory" } else { "file" };
        
        if entry.is_directory {
            let encoded_path = urlencoding::encode(&entry.path);
            html.push_str(&format!(
                "<div class=\"entry\"><a href=\"/?path={}\"><span class=\"icon\">{}</span><span class=\"{}\">{}</span></a></div>",
                encoded_path, icon, class, entry.name
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

async fn index(Query(params): Query<HashMap<String, String>>, State(state): State<AppState>) -> Result<Html<String>, (StatusCode, String)> {
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

async fn handler_404() -> (StatusCode, &'static str) {
    (StatusCode::NOT_FOUND, "not found")
}

fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .fallback(handler_404)
        .with_state(state)
}

pub async fn start_server(target_dir: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let state = AppState { target_dir };
    let app = create_router(state)
        .layer(OtelInResponseLayer::default())
        .layer(OtelAxumLayer::default());

    let listener = TcpListener::bind("0.0.0.0:5420").await?;
    info!("Web server listening on http://0.0.0.0:5420, press Ctrl+C to stop");

    axum::serve(listener, app).await?;

    Ok(())
}
