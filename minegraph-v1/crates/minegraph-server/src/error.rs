//! Structured API error type.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// API error that converts to a proper HTTP response.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid signature")]
    InvalidSignature,
    #[error("identity not registered: {0}")]
    UnregisteredIdentity(String),
    #[error("internal error: {0}")]
    Internal(String),
    #[error("store error: {0}")]
    Store(#[from] minegraph_store::StoreError),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            ApiError::InvalidSignature => {
                (StatusCode::BAD_REQUEST, "invalid signature".to_string())
            }
            ApiError::UnregisteredIdentity(kid) => (
                StatusCode::BAD_REQUEST,
                format!("identity not registered: {kid}"),
            ),
            ApiError::Internal(msg) => {
                tracing::error!("internal error: {msg}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".to_string(),
                )
            }
            ApiError::Store(e) => {
                tracing::error!("store error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".to_string(),
                )
            }
        };

        let body = axum::Json(json!({
            "error": message,
            "status": status.as_u16(),
        }));
        (status, body).into_response()
    }
}
