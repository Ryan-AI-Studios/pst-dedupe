//! HTTP JSON error mapping for matter-service.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use matter_core::Error as CoreError;
use serde::Serialize;

/// Machine-readable error body.
#[derive(Debug, Clone, Serialize)]
pub struct ApiErrorBody {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by_user: Option<String>,
}

/// Service-layer error with HTTP status.
///
/// Body is boxed so `Result<T, ApiError>` stays clippy-friendly (`result_large_err`).
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub body: Box<ApiErrorBody>,
}

impl ApiError {
    pub fn new(status: StatusCode, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status,
            body: Box::new(ApiErrorBody {
                code: code.into(),
                message: message.into(),
                expected: None,
                actual: None,
                item_id: None,
                by_user: None,
            }),
        }
    }

    pub fn from_core(err: CoreError) -> Self {
        match err {
            CoreError::VersionConflict { expected, actual } => Self {
                status: StatusCode::CONFLICT,
                body: Box::new(ApiErrorBody {
                    code: "version_conflict".into(),
                    message: format!("version conflict: expected {expected}, actual {actual}"),
                    expected: Some(expected),
                    actual: Some(actual),
                    item_id: None,
                    by_user: None,
                }),
            },
            CoreError::Locked { item_id, by_user } => Self {
                status: StatusCode::CONFLICT,
                body: Box::new(ApiErrorBody {
                    code: "locked".into(),
                    message: format!("item {item_id} is locked by {by_user}"),
                    expected: None,
                    actual: None,
                    item_id: Some(item_id),
                    by_user: Some(by_user),
                }),
            },
            CoreError::Conflict { message } => Self::new(StatusCode::CONFLICT, "conflict", message),
            CoreError::Unauthorized(message) => {
                Self::new(StatusCode::UNAUTHORIZED, "unauthorized", message)
            }
            CoreError::Forbidden(message) => Self::new(StatusCode::FORBIDDEN, "forbidden", message),
            CoreError::ItemNotFound(id) => {
                Self::new(StatusCode::NOT_FOUND, "not_found", format!("item {id}"))
            }
            CoreError::MatterAlreadyOpen(message) => {
                Self::new(StatusCode::CONFLICT, "matter_already_open", message)
            }
            other => Self::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                other.to_string(),
            ),
        }
    }
}

impl From<CoreError> for ApiError {
    fn from(value: CoreError) -> Self {
        Self::from_core(value)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(*self.body)).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
