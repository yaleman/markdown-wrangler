use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

pub(crate) enum WebError {
    BadRequest(String),
    NotFound(String),
    InternalError(String),
    Unauthorized,
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        match self {
            WebError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
            WebError::NotFound(msg) => (StatusCode::NOT_FOUND, msg).into_response(),
            WebError::InternalError(msg) => {
                (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response()
            }
            WebError::Unauthorized => {
                (StatusCode::UNAUTHORIZED, "Unauthorized access".to_string()).into_response()
            }
        }
    }
}

impl From<std::io::Error> for WebError {
    fn from(err: std::io::Error) -> Self {
        WebError::InternalError(format!("IO error: {err}"))
    }
}
