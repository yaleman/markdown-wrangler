use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

pub(crate) enum WebError {
    BadRequest(String),
    NotFound(String),
    Internal(String),
    Unauthorized,
    Forbidden(String),
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        match self {
            WebError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
            WebError::NotFound(msg) => (StatusCode::NOT_FOUND, msg).into_response(),
            WebError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response(),
            WebError::Unauthorized => {
                (StatusCode::UNAUTHORIZED, "Unauthorized access".to_string()).into_response()
            }
            WebError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg).into_response(),
        }
    }
}

impl From<std::io::Error> for WebError {
    fn from(err: std::io::Error) -> Self {
        WebError::Internal(format!("IO error: {err}"))
    }
}
