//! Axum routes for the multi-user matter service (+ optional platform OIDC).

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use matter_core::{
    privilege_status, ApplyCodesInput, UpsertItemPrivilegeInput, UpsertNoteInput, ROLE_ADMIN,
    ROLE_READ_ONLY, ROLE_REVIEWER,
};
use serde::{Deserialize, Serialize};

use crate::auth::{AuthUser, OptionalAuthUser};
use crate::error::{ApiError, ApiResult};
use crate::oidc::{complete_oidc_login, pkce_challenge_s256, random_urlsafe, PendingLogin};
use crate::state::{PlatformState, WriteGate};

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub gate: WriteGate,
    /// Present when serving with `--platform` (track 0059).
    pub platform: Option<PlatformState>,
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/login", post(login))
        .route("/v1/logout", post(logout))
        .route("/v1/oidc/login", get(oidc_login))
        .route("/v1/oidc/callback", get(oidc_callback))
        .route("/v1/oidc/logout", post(logout))
        .route("/v1/tenants/me", get(tenants_me))
        .route("/v1/platform/matters", get(platform_matters))
        .route("/v1/users", get(list_users).post(create_user))
        .route("/v1/users/{id}/disable", post(disable_user))
        .route("/v1/items", get(list_items))
        .route("/v1/items/{id}", get(get_item))
        .route("/v1/items/{id}/body", get(get_item_body))
        .route("/v1/items/{id}/codes", post(apply_codes))
        .route("/v1/items/{id}/notes", post(upsert_note))
        .route("/v1/items/{id}/privilege", post(upsert_privilege))
        .route("/v1/items/{id}/lock", post(lock_item).delete(unlock_item))
        .route("/v1/items/{id}/force-unlock", post(force_unlock_item))
        .route("/v1/batches", post(create_batch))
        .route("/v1/batches/{id}/checkout", post(checkout_batch))
        .route("/v1/batches/{id}/checkin", post(checkin_batch))
        .route("/v1/batches/{id}/items", get(list_batch_items))
        .route("/v1/qc/samples", post(create_qc_sample))
        .route("/v1/qc/samples/{sample_id}", get(get_qc_sample_report))
        .route(
            "/v1/qc/samples/{sample_id}/items/{item_id}",
            post(record_qc_outcome),
        )
        .with_state(state)
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub name: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: UserDto,
    pub expires_at: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct UserDto {
    pub id: String,
    pub display_name: String,
    pub role: String,
    pub disabled_at: Option<String>,
}

impl From<matter_core::MatterUser> for UserDto {
    fn from(u: matter_core::MatterUser) -> Self {
        Self {
            id: u.id,
            display_name: u.display_name,
            role: u.role,
            disabled_at: u.disabled_at,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub name: String,
    pub password: String,
    pub role: String,
}

#[derive(Debug, Serialize)]
pub struct ItemThin {
    pub id: String,
    pub subject: Option<String>,
    pub from_addr: Option<String>,
    pub sent_at: Option<String>,
    pub review_version: i64,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct MutateCodesRequest {
    pub add_code_ids: Option<Vec<String>>,
    pub remove_code_ids: Option<Vec<String>>,
    pub propagate_family: Option<bool>,
    pub expected_version: Option<i64>,
    /// Ignored under service strict actor mode (session user is injected).
    #[serde(default)]
    pub actor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MutateCodesResponse {
    pub target_item_ids: Vec<String>,
    pub review_versions: Vec<i64>,
}

#[derive(Debug, Deserialize)]
pub struct UpsertNoteRequest {
    pub body: String,
    pub id: Option<String>,
    pub highlight_id: Option<String>,
    pub expected_version: Option<i64>,
    #[serde(default)]
    pub actor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NoteResponse {
    pub id: String,
    pub item_id: String,
    pub body: String,
    pub review_version: i64,
    pub created_by: String,
    pub updated_by: String,
}

#[derive(Debug, Deserialize)]
pub struct PrivilegeRequest {
    pub basis: String,
    pub description: Option<String>,
    pub status: Option<String>,
    pub withhold: Option<bool>,
    pub include_on_log: Option<bool>,
    pub expected_version: Option<i64>,
    #[serde(default)]
    pub actor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LockRequest {
    pub reason: Option<String>,
    pub ttl_hours: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateBatchRequest {
    pub name: String,
    pub item_ids: Vec<String>,
    pub filter_json: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BatchDto {
    pub id: String,
    pub name: String,
    pub created_by: String,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct BatchItemsQuery {
    pub after: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct ListItemsQuery {
    pub limit: Option<usize>,
    pub after: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateQcSampleRequest {
    pub name: String,
    pub sample_pct: Option<f64>,
    pub sample_n: Option<i64>,
    pub seed: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct QcSampleResponse {
    pub id: String,
    pub name: String,
    pub seed: i64,
    pub item_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct RecordQcRequest {
    pub outcome: String,
    pub notes: Option<String>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn healthz() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true }))
}

async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> ApiResult<Json<LoginResponse>> {
    if let Some(ps) = &state.platform {
        if ps.oidc_required {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "oidc_required",
                "password login disabled; use OIDC SSO for this tenant",
            ));
        }
    }
    let matter = state.gate.lock().await;
    let issue = matter
        .authenticate(&body.name, &body.password)
        .map_err(ApiError::from)?;
    Ok(Json(LoginResponse {
        token: issue.token,
        user: issue.user.into(),
        expires_at: issue.expires_at,
    }))
}

/// P0: invalidate session and release that user's item locks + batch checkouts.
async fn logout(State(state): State<AppState>, opt: OptionalAuthUser) -> ApiResult<StatusCode> {
    let matter = state.gate.lock().await;
    if let Some(user) = opt.user {
        matter
            .logout_user_release_locks(&user.id)
            .map_err(ApiError::from)?;
    } else if let Some(t) = opt.token.as_deref() {
        // Best-effort: drop the single session even if already expired.
        matter.invalidate_session(t).map_err(ApiError::from)?;
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct OidcLoginQuery {
    tenant: Option<String>,
    /// When `json` or Accept: application/json, return authorize URL as JSON (tests/headless).
    format: Option<String>,
    redirect_uri: Option<String>,
}

#[derive(Debug, Serialize)]
struct OidcLoginJson {
    authorize_url: String,
    state: String,
    /// Returned for mock/test flows only (also stored server-side).
    #[serde(skip_serializing_if = "Option::is_none")]
    code_verifier: Option<String>,
    nonce: String,
}

async fn oidc_login(
    State(state): State<AppState>,
    Query(q): Query<OidcLoginQuery>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let ps = state.platform.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "platform mode not enabled",
        )
    })?;
    let want_json = q
        .format
        .as_deref()
        .map(|f| f.eq_ignore_ascii_case("json"))
        .unwrap_or(false)
        || headers
            .get(header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .map(|a| a.contains("application/json"))
            .unwrap_or(false);

    let platform = ps.platform.lock().await;
    let tenant = match q.tenant.as_deref() {
        Some(slug) => platform
            .get_tenant_by_slug(slug)
            .map_err(|e| {
                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "platform", e.to_string())
            })?
            .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_found", "tenant not found"))?,
        None => platform
            .get_tenant_by_id(&ps.tenant_id)
            .map_err(|e| {
                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "platform", e.to_string())
            })?
            .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_found", "tenant not found"))?,
    };
    // Cross-tenant login against a matter hosted for another tenant → 404.
    if tenant.id != ps.tenant_id {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "tenant not found",
        ));
    }
    let idp = platform
        .get_idp_config(&tenant.id)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "platform", e.to_string()))?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "idp_missing",
                "IdP not configured for tenant",
            )
        })?;
    if !idp.enabled {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "idp_disabled",
            "IdP disabled",
        ));
    }

    let state_tok = random_urlsafe(32);
    let nonce = random_urlsafe(24);
    let code_verifier = random_urlsafe(48);
    let challenge = pkce_challenge_s256(&code_verifier);
    // Exact allowlist: only the service callback URI (ignore client-supplied redirect).
    let redirect_uri = crate::oidc::canonical_callback_uri(&ps.public_base);
    if let Some(req) = q.redirect_uri.as_deref() {
        crate::oidc::assert_redirect_allowed(&ps.public_base, req)?;
    }

    let expires = chrono::Utc::now() + chrono::Duration::seconds(600);
    platform
        .store_oidc_pending(
            &state_tok,
            &tenant.id,
            &code_verifier,
            &nonce,
            &redirect_uri,
            600,
        )
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "platform", e.to_string()))?;
    {
        let mut mem = ps.oidc.memory_pending.lock().await;
        mem.insert(
            state_tok.clone(),
            PendingLogin {
                tenant_id: tenant.id.clone(),
                code_verifier: code_verifier.clone(),
                nonce: nonce.clone(),
                redirect_uri: redirect_uri.clone(),
                expires_at: expires,
            },
        );
    }

    let authorize_url = ps
        .oidc
        .provider
        .start_authorization(&idp, &redirect_uri, &state_tok, &nonce, &challenge)
        .await?;

    if want_json {
        // PKCE verifier is server-side secret — only expose under mock IdP for headless tests.
        let expose_verifier = ps.oidc.is_mock();
        return Ok(Json(OidcLoginJson {
            authorize_url,
            state: state_tok,
            code_verifier: if expose_verifier {
                Some(code_verifier)
            } else {
                None
            },
            nonce,
        })
        .into_response());
    }
    Ok(Redirect::temporary(&authorize_url).into_response())
}

#[derive(Debug, Deserialize)]
struct OidcCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    /// Test-only: when using mock IdP, callers may pass claims via minted code.
    #[serde(default)]
    format: Option<String>,
}

async fn oidc_callback(
    State(state): State<AppState>,
    Query(q): Query<OidcCallbackQuery>,
) -> ApiResult<Json<LoginResponse>> {
    let ps = state.platform.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "platform mode not enabled",
        )
    })?;
    if let Some(err) = q.error {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "oidc_error", err));
    }
    let code = q.code.as_deref().filter(|s| !s.is_empty()).ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "missing_code",
            "missing authorization code",
        )
    })?;
    let state_tok = q
        .state
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "missing_state",
                "missing OIDC state",
            )
        })?;

    // Single-use state: always consume both stores so a replay cannot fall back.
    let pending_mem = {
        let mut mem = ps.oidc.memory_pending.lock().await;
        mem.remove(state_tok)
    };
    let pending_db = {
        let platform = ps.platform.lock().await;
        // Best-effort delete even when memory already held the state.
        platform.take_oidc_pending(state_tok).ok()
    };
    let (tenant_id, code_verifier, nonce, redirect_uri) = if let Some(p) = pending_mem {
        if p.expires_at < chrono::Utc::now() {
            return Err(ApiError::new(
                StatusCode::UNAUTHORIZED,
                "oidc_state",
                "OIDC state expired",
            ));
        }
        (p.tenant_id, p.code_verifier, p.nonce, p.redirect_uri)
    } else if let Some(p) = pending_db {
        // platform OidcPending stores expires_at as RFC3339 string.
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        if p.expires_at.as_str() <= now.as_str() {
            return Err(ApiError::new(
                StatusCode::UNAUTHORIZED,
                "oidc_state",
                "OIDC state expired",
            ));
        }
        (p.tenant_id, p.code_verifier, p.nonce, p.redirect_uri)
    } else {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "oidc_state",
            "invalid or already-used OIDC state",
        ));
    };

    if tenant_id != ps.tenant_id {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "tenant not found",
        ));
    }

    let (idp, client_secret) = {
        let platform = ps.platform.lock().await;
        let idp = platform
            .get_idp_config(&tenant_id)
            .map_err(|e| {
                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "platform", e.to_string())
            })?
            .ok_or_else(|| {
                ApiError::new(StatusCode::BAD_REQUEST, "idp_missing", "IdP not configured")
            })?;
        let secret = if ps.oidc.is_mock() {
            // Mock IdP does not use the client secret; allow missing env/ciphertext.
            platform
                .resolve_client_secret(&tenant_id)
                .unwrap_or_else(|_| String::new())
        } else {
            platform.resolve_client_secret(&tenant_id).map_err(|e| {
                ApiError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "secret_unavailable",
                    format!("client secret unavailable: {e}"),
                )
            })?
        };
        (idp, secret)
    };

    let claims = ps
        .oidc
        .provider
        .finish_authorization(
            &idp,
            &redirect_uri,
            code,
            &code_verifier,
            &nonce,
            &client_secret,
        )
        .await?;

    let platform = ps.platform.lock().await;
    let matter = state.gate.lock().await;
    let issue = complete_oidc_login(&platform, &matter, &tenant_id, &claims)?;
    let _ = q.format; // reserved for future HTML vs JSON
    Ok(Json(LoginResponse {
        token: issue.token,
        user: issue.user.into(),
        expires_at: issue.expires_at,
    }))
}

async fn tenants_me(
    State(state): State<AppState>,
    auth: AuthUser,
) -> ApiResult<Json<serde_json::Value>> {
    auth.require_read()?;
    let ps = state.platform.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "platform mode not enabled",
        )
    })?;
    let platform = ps.platform.lock().await;
    let tenant = platform
        .get_tenant_by_id(&ps.tenant_id)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "platform", e.to_string()))?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_found", "tenant not found"))?;
    Ok(Json(serde_json::json!({
        "id": tenant.id,
        "slug": tenant.slug,
        "display_name": tenant.display_name,
        "status": tenant.status,
        "jit_provision": tenant.jit_provision,
        "oidc_required": tenant.oidc_required,
    })))
}

async fn platform_matters(
    State(state): State<AppState>,
    auth: AuthUser,
) -> ApiResult<Json<serde_json::Value>> {
    auth.require_read()?;
    let ps = state.platform.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "platform mode not enabled",
        )
    })?;
    let platform = ps.platform.lock().await;
    let matters = platform
        .list_matters(&ps.tenant_id)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "platform", e.to_string()))?;
    Ok(Json(serde_json::json!(matters
        .into_iter()
        .map(|m| serde_json::json!({
            "matter_id": m.matter_id,
            "storage_root": m.storage_root,
            "status": m.status,
            "registered_at": m.registered_at,
        }))
        .collect::<Vec<_>>())))
}

async fn list_users(
    State(state): State<AppState>,
    auth: AuthUser,
) -> ApiResult<Json<Vec<UserDto>>> {
    auth.require_admin()?;
    let matter = state.gate.lock().await;
    let users = matter.list_users().map_err(ApiError::from)?;
    Ok(Json(users.into_iter().map(UserDto::from).collect()))
}

async fn create_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateUserRequest>,
) -> ApiResult<(StatusCode, Json<UserDto>)> {
    auth.require_admin()?;
    let matter = state.gate.lock().await;
    let user = matter
        .create_user(&body.name, &body.role, &body.password, auth.id())
        .map_err(ApiError::from)?;
    Ok((StatusCode::CREATED, Json(user.into())))
}

async fn disable_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    auth.require_admin()?;
    let matter = state.gate.lock().await;
    matter
        .disable_user(&id, auth.id())
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_items(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(q): Query<ListItemsQuery>,
) -> ApiResult<Json<Vec<ItemThin>>> {
    auth.require_read()?;
    let matter = state.gate.lock().await;
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    // Batch-constrained when caller has active checkout (spec §3.4.6).
    let items = matter
        .list_items_thin_for_user(Some(auth.id()), q.after.as_deref(), limit)
        .map_err(ApiError::from)?;
    Ok(Json(
        items
            .into_iter()
            .map(|r| ItemThin {
                id: r.id,
                subject: r.subject,
                from_addr: r.from_addr,
                sent_at: r.sent_at,
                review_version: r.review_version,
                status: r.status,
            })
            .collect(),
    ))
}

async fn get_item(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<ItemThin>> {
    auth.require_read()?;
    let matter = state.gate.lock().await;
    matter
        .assert_user_can_view_item(auth.id(), &id)
        .map_err(ApiError::from)?;
    let item = matter.get_item(&id).map_err(ApiError::from)?;
    let review_version = matter.get_review_version(&id).map_err(ApiError::from)?;
    Ok(Json(ItemThin {
        id: item.id,
        subject: item.subject,
        from_addr: item.from_addr,
        sent_at: item.sent_at,
        review_version,
        status: item.status,
    }))
}

/// Review body (plain text preferred, HTML fallback) for multi-user clients.
async fn get_item_body(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    auth.require_read()?;
    let matter = state.gate.lock().await;
    matter
        .assert_user_can_view_item(auth.id(), &id)
        .map_err(ApiError::from)?;
    let body = matter
        .load_item_body_for_service(&id, 2 * 1024 * 1024)
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({
        "item_id": body.item_id,
        "content_type": body.content_type,
        "text": body.text,
        "digest": body.digest,
        "review_version": body.review_version,
        "truncated": body.truncated,
    })))
}

async fn apply_codes(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<MutateCodesRequest>,
) -> ApiResult<Json<MutateCodesResponse>> {
    auth.require_reviewer()?;
    let expected = body.expected_version.ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "missing_version",
            "expected_version is required",
        )
    })?;
    // Body actor intentionally ignored.
    let _spoof = body.actor;
    let matter = state.gate.lock().await;
    let result = matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![id],
            add_code_ids: body.add_code_ids.unwrap_or_default(),
            remove_code_ids: body.remove_code_ids.unwrap_or_default(),
            propagate_family: body.propagate_family.unwrap_or(false),
            actor: auth.id().to_string(),
            expected_version: Some(expected),
        })
        .map_err(ApiError::from)?;
    Ok(Json(MutateCodesResponse {
        target_item_ids: result.target_item_ids,
        review_versions: result.review_versions,
    }))
}

async fn upsert_note(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<UpsertNoteRequest>,
) -> ApiResult<Json<NoteResponse>> {
    auth.require_reviewer()?;
    let expected = body.expected_version.ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "missing_version",
            "expected_version is required",
        )
    })?;
    let _spoof = body.actor;
    let matter = state.gate.lock().await;
    let note = matter
        .upsert_note(UpsertNoteInput {
            id: body.id,
            item_id: id,
            body: body.body,
            highlight_id: body.highlight_id,
            actor: auth.id().to_string(),
            expected_version: Some(expected),
        })
        .map_err(ApiError::from)?;
    let review_version = matter
        .get_review_version(&note.item_id)
        .map_err(ApiError::from)?;
    Ok(Json(NoteResponse {
        id: note.id,
        item_id: note.item_id,
        body: note.body,
        review_version,
        created_by: note.created_by,
        updated_by: note.updated_by,
    }))
}

async fn upsert_privilege(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<PrivilegeRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    auth.require_reviewer()?;
    let expected = body.expected_version.ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "missing_version",
            "expected_version is required",
        )
    })?;
    let _spoof = body.actor;
    let matter = state.gate.lock().await;
    let row = matter
        .upsert_item_privilege(UpsertItemPrivilegeInput {
            item_id: id.clone(),
            basis: body.basis,
            description: body.description.unwrap_or_default(),
            status: body
                .status
                .unwrap_or_else(|| privilege_status::ASSERTED.to_string()),
            withhold: body.withhold.unwrap_or(true),
            include_on_log: body.include_on_log.unwrap_or(true),
            actor: auth.id().to_string(),
            expected_version: Some(expected),
        })
        .map_err(ApiError::from)?;
    let review_version = matter.get_review_version(&id).map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({
        "item_id": row.item_id,
        "basis": row.basis,
        "status": row.status,
        "withhold": row.withhold,
        "review_version": review_version,
        "updated_by": row.updated_by,
    })))
}

async fn lock_item(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    body: Option<Json<LockRequest>>,
) -> ApiResult<Json<serde_json::Value>> {
    auth.require_reviewer()?;
    let reason = body.as_ref().and_then(|b| b.reason.clone());
    let ttl = body.as_ref().and_then(|b| b.ttl_hours);
    let matter = state.gate.lock().await;
    let lock = matter
        .lock_item(&id, auth.id(), reason.as_deref(), ttl)
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({
        "item_id": lock.item_id,
        "user_id": lock.user_id,
        "locked_at": lock.locked_at,
        "expires_at": lock.expires_at,
        "reason": lock.reason,
    })))
}

async fn unlock_item(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    auth.require_reviewer()?;
    let matter = state.gate.lock().await;
    matter.unlock_item(&id, auth.id()).map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Admin force-release of any holder's item lock (audited).
///
/// Available while the service holds the exclusive matter lock — operators do
/// not need a second write-open of the matter folder.
async fn force_unlock_item(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    auth.require_admin()?;
    let matter = state.gate.lock().await;
    matter
        .force_unlock(&id, auth.id())
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn create_batch(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateBatchRequest>,
) -> ApiResult<(StatusCode, Json<BatchDto>)> {
    auth.require_reviewer()?;
    let matter = state.gate.lock().await;
    let batch = matter
        .create_batch(
            &body.name,
            &body.item_ids,
            auth.id(),
            body.filter_json.as_deref(),
        )
        .map_err(ApiError::from)?;
    Ok((
        StatusCode::CREATED,
        Json(BatchDto {
            id: batch.id,
            name: batch.name,
            created_by: batch.created_by,
            status: batch.status,
        }),
    ))
}

async fn checkout_batch(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    auth.require_reviewer()?;
    let matter = state.gate.lock().await;
    let co = matter
        .checkout_batch(&id, auth.id())
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({
        "batch_id": co.batch_id,
        "user_id": co.user_id,
        "checked_out_at": co.checked_out_at,
    })))
}

async fn checkin_batch(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    auth.require_reviewer()?;
    let matter = state.gate.lock().await;
    matter
        .checkin_batch(&id, auth.id())
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_batch_items(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Query(q): Query<BatchItemsQuery>,
) -> ApiResult<Json<Vec<serde_json::Value>>> {
    auth.require_read()?;
    let matter = state.gate.lock().await;
    let rows = matter
        .list_batch_items(&id, q.after.as_deref(), q.limit.unwrap_or(100))
        .map_err(ApiError::from)?;
    // Membership is authoritative — no FilterSpec escape hatch here.
    let out: Vec<_> = rows
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "item_id": r.item_id,
                "review_version": r.review_version,
                "subject": r.subject,
                "from_addr": r.from_addr,
                "sent_at": r.sent_at,
            })
        })
        .collect();
    Ok(Json(out))
}

async fn create_qc_sample(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateQcSampleRequest>,
) -> ApiResult<(StatusCode, Json<QcSampleResponse>)> {
    if auth.require_admin().is_err() {
        auth.require_reviewer()?;
    }
    let matter = state.gate.lock().await;
    let seed = body.seed.unwrap_or(42);
    let (sample, items) = matter
        .create_qc_sample(&body.name, auth.id(), body.sample_pct, body.sample_n, seed)
        .map_err(ApiError::from)?;
    Ok((
        StatusCode::CREATED,
        Json(QcSampleResponse {
            id: sample.id,
            name: sample.name,
            seed: sample.seed,
            item_ids: items.into_iter().map(|i| i.item_id).collect(),
        }),
    ))
}

async fn record_qc_outcome(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((sample_id, item_id)): Path<(String, String)>,
    Json(body): Json<RecordQcRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    auth.require_reviewer()?;
    let matter = state.gate.lock().await;
    let row = matter
        .record_qc_outcome(
            &sample_id,
            &item_id,
            &body.outcome,
            body.notes.as_deref(),
            auth.id(),
        )
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({
        "sample_id": row.sample_id,
        "item_id": row.item_id,
        "outcome": row.outcome,
        "recorded_by": row.recorded_by,
        "recorded_at": row.recorded_at,
    })))
}

/// JSON summary report of a QC sample and recorded outcomes.
async fn get_qc_sample_report(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(sample_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    auth.require_read()?;
    let matter = state.gate.lock().await;
    let (sample, items) = matter
        .qc_sample_report(&sample_id)
        .map_err(ApiError::from)?;
    let mut agree = 0u64;
    let mut disagree = 0u64;
    let mut corrected = 0u64;
    let mut pending = 0u64;
    for it in &items {
        match it.outcome.as_deref() {
            Some("agree") => agree += 1,
            Some("disagree") => disagree += 1,
            Some("corrected") => corrected += 1,
            _ => pending += 1,
        }
    }
    Ok(Json(serde_json::json!({
        "sample": {
            "id": sample.id,
            "name": sample.name,
            "created_by": sample.created_by,
            "sample_pct": sample.sample_pct,
            "sample_n": sample.sample_n,
            "seed": sample.seed,
            "created_at": sample.created_at,
            "status": sample.status,
        },
        "summary": {
            "total": items.len(),
            "agree": agree,
            "disagree": disagree,
            "corrected": corrected,
            "pending": pending,
        },
        "items": items.iter().map(|i| serde_json::json!({
            "item_id": i.item_id,
            "primary_coder": i.primary_coder,
            "outcome": i.outcome,
            "notes": i.notes,
            "recorded_by": i.recorded_by,
            "recorded_at": i.recorded_at,
        })).collect::<Vec<_>>(),
    })))
}

// Silence unused role constant warnings if not referenced in routes.
#[allow(dead_code)]
fn _role_consts() {
    let _ = (ROLE_ADMIN, ROLE_REVIEWER, ROLE_READ_ONLY);
}
