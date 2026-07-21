//! Bearer auth extraction — injects session user; never trusts body actor.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use matter_core::{require_role, MatterUser, ROLE_ADMIN, ROLE_READ_ONLY, ROLE_REVIEWER};

use crate::error::{ApiError, ApiResult};
use crate::routes::AppState;

/// Authenticated session user extracted from `Authorization: Bearer …`.
#[derive(Debug, Clone)]
pub struct AuthUser(pub MatterUser);

impl AuthUser {
    pub fn id(&self) -> &str {
        &self.0.id
    }

    pub fn require_reviewer(&self) -> ApiResult<()> {
        require_role(&self.0, ROLE_REVIEWER).map_err(ApiError::from)
    }

    pub fn require_admin(&self) -> ApiResult<()> {
        require_role(&self.0, ROLE_ADMIN).map_err(ApiError::from)
    }

    pub fn require_read(&self) -> ApiResult<()> {
        require_role(&self.0, ROLE_READ_ONLY).map_err(ApiError::from)
    }
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                ApiError::new(
                    axum::http::StatusCode::UNAUTHORIZED,
                    "unauthorized",
                    "missing Authorization header",
                )
            })?;
        let token = header
            .strip_prefix("Bearer ")
            .or_else(|| header.strip_prefix("bearer "))
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| {
                ApiError::new(
                    axum::http::StatusCode::UNAUTHORIZED,
                    "unauthorized",
                    "expected Bearer token",
                )
            })?;
        let matter = state.gate.lock().await;
        let user = matter.resolve_session(token).map_err(ApiError::from)?;
        Ok(AuthUser(user))
    }
}
