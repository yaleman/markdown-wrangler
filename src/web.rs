use axum::{Router, http::StatusCode, response::Html, routing::get};
use axum_tracing_opentelemetry::middleware::{OtelAxumLayer, OtelInResponseLayer};
use tokio::net::TcpListener;
use tracing::info;

async fn index() -> Html<&'static str> {
    Html("Hello World")
}

async fn handler_404() -> (StatusCode, &'static str) {
    (StatusCode::NOT_FOUND, "not found")
}

fn create_router() -> Router {
    Router::new().route("/", get(index)).fallback(handler_404)
}

pub async fn start_server() -> Result<(), Box<dyn std::error::Error>> {
    let app = create_router()
        .layer(OtelInResponseLayer::default())
        .layer(OtelAxumLayer::default());

    let listener = TcpListener::bind("0.0.0.0:5420").await?;
    info!("Web server listening on http://0.0.0.0:5420, press Ctrl+C to stop");

    axum::serve(listener, app).await?;

    Ok(())
}
