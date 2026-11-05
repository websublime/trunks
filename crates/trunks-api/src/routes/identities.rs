//! `/api/v0/users*` (enterprise, admin) and `/api/v0/clients*` (enterprise; admins see every
//! client, users their own and their teams' — D14 self-service).
//!
//! Tokens are serialized exactly twice: on creation and on rotation. Listings never carry them.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Extension, Json, Router};
use trunks_core::Error;
use trunks_core::access::SubjectKind;
use trunks_core::domain::{McpClientRecord, UserRecord};
use trunks_core::identity::UserPrincipal;
use trunks_types::api::{
    CreateMcpClientRequest, CreateUserRequest, McpClient, McpClientWithToken, Owner,
    UpdateMcpClientRequest, UpdateUserRequest, User, UserWithToken,
};
use trunks_types::error::messages;

use crate::error::{ApiError, JsonBody};
use crate::state::AppState;

/// Routes behind `require_enterprise` + `require_admin`.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/users", get(list_users).post(create_user))
        .route("/users/{username}", axum::routing::put(update_user).delete(delete_user))
}

/// Routes behind `require_enterprise` only: ownership is checked per handler.
pub fn clients_router() -> Router<AppState> {
    Router::new()
        .route("/clients", get(list_clients).post(create_client))
        .route("/clients/{name}", axum::routing::put(update_client).delete(delete_client))
}

async fn owner_view(st: &AppState, c: &McpClientRecord) -> Result<Option<Owner>, ApiError> {
    let Some(subject) = c.owner else { return Ok(None) };
    Ok(st
        .identity
        .subject_name(subject)
        .await?
        .map(|name| Owner { kind: subject.0.as_str().to_owned(), name }))
}

async fn client_view(st: &AppState, c: &McpClientRecord) -> Result<McpClient, ApiError> {
    let mut v = c.to_api();
    v.owner = owner_view(st, c).await?;
    Ok(v)
}

/// Resolves the `owner` of a request against the caller: admins may pick anyone (or none);
/// users get themselves unless they name a team they maintain.
async fn resolve_owner(
    st: &AppState,
    p: &UserPrincipal,
    requested: Option<&Owner>,
) -> Result<Option<(SubjectKind, i64)>, ApiError> {
    let requested = match requested {
        Some(o) => {
            let kind = SubjectKind::parse(&o.kind)
                .ok_or_else(|| Error::invalid_input(messages::INVALID_SUBJECT_KIND))?;
            Some((kind, st.identity.subject_id(kind, &o.name).await?))
        }
        None => None,
    };
    if p.is_admin() {
        return Ok(requested);
    }
    let subject = requested.unwrap_or_else(|| (SubjectKind::User, p.user_id().unwrap_or(0)));
    if !p.may_act_for(subject) {
        return Err(ApiError(Error::Forbidden { reason: messages::USER_NOT_AUTHORIZED.into() }));
    }
    Ok(Some(subject))
}

/// Loads a client the caller may manage (admin: any; user: own or maintained team's).
async fn managed_client(
    st: &AppState,
    p: &UserPrincipal,
    name: &str,
) -> Result<McpClientRecord, ApiError> {
    let client = st
        .identity
        .client_by_name(name)
        .await?
        .ok_or_else(|| Error::not_found(format!("MCP client '{name}'")))?;
    if p.is_admin() || client.owner.is_some_and(|o| p.may_act_for(o)) {
        Ok(client)
    } else {
        Err(ApiError(Error::Forbidden { reason: messages::NOT_OWNER.into() }))
    }
}

async fn list_users(State(st): State<AppState>) -> Result<Json<Vec<User>>, ApiError> {
    Ok(Json(st.identity.users().await?.iter().map(UserRecord::to_api).collect()))
}

async fn create_user(
    State(st): State<AppState>,
    JsonBody(body): JsonBody<CreateUserRequest>,
) -> Result<(StatusCode, Json<UserWithToken>), ApiError> {
    let (user, token) =
        st.identity.create_user(&body.username, body.access_token.as_deref()).await?;
    Ok((
        StatusCode::CREATED,
        Json(UserWithToken { username: user.username, role: user.role, access_token: token }),
    ))
}

async fn update_user(
    State(st): State<AppState>,
    Path(username): Path<String>,
    JsonBody(body): JsonBody<UpdateUserRequest>,
) -> Result<Json<UserWithToken>, ApiError> {
    let (user, token) =
        st.identity.update_user_token(&username, body.access_token.as_deref()).await?;
    Ok(Json(UserWithToken { username: user.username, role: user.role, access_token: token }))
}

async fn delete_user(
    State(st): State<AppState>,
    Path(username): Path<String>,
) -> Result<StatusCode, ApiError> {
    st.identity.delete_user(&username).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_clients(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
) -> Result<Json<Vec<McpClient>>, ApiError> {
    let records = if p.is_admin() {
        st.identity.clients().await?
    } else {
        st.identity.clients_owned_by(&p.subjects()).await?
    };
    let mut out = Vec::with_capacity(records.len());
    for c in &records {
        out.push(client_view(&st, c).await?);
    }
    Ok(Json(out))
}

async fn create_client(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
    JsonBody(body): JsonBody<CreateMcpClientRequest>,
) -> Result<(StatusCode, Json<McpClientWithToken>), ApiError> {
    let owner = resolve_owner(&st, &p, body.owner.as_ref()).await?;
    let (client, token) = st
        .identity
        .create_client(
            &body.name,
            &body.description,
            &body.allow_list,
            body.access_token.as_deref(),
            owner,
        )
        .await?;
    let owner = owner_view(&st, &client).await?;
    Ok((
        StatusCode::CREATED,
        Json(McpClientWithToken {
            name: client.name,
            description: client.description,
            allow_list: client.allow_list,
            owner,
            access_token: Some(token),
        }),
    ))
}

async fn update_client(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
    Path(name): Path<String>,
    JsonBody(body): JsonBody<UpdateMcpClientRequest>,
) -> Result<Json<McpClientWithToken>, ApiError> {
    managed_client(&st, &p, &name).await?;
    let new_token = match (&body.access_token, body.rotate_token) {
        (Some(t), _) => Some(Some(t.as_str())),
        (None, true) => Some(None),
        (None, false) => None,
    };
    let (client, token) = st
        .identity
        .update_client(&name, body.description.as_deref(), body.allow_list.as_deref(), new_token)
        .await?;
    let owner = owner_view(&st, &client).await?;
    Ok(Json(McpClientWithToken {
        name: client.name,
        description: client.description,
        allow_list: client.allow_list,
        owner,
        access_token: token,
    }))
}

async fn delete_client(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
    Path(name): Path<String>,
) -> Result<StatusCode, ApiError> {
    match managed_client(&st, &p, &name).await {
        Ok(_) => {}
        // idempotent for admins; users get the ownership error
        Err(ApiError(Error::NotFound { .. })) if p.is_admin() => return Ok(StatusCode::NO_CONTENT),
        Err(e) => return Err(e),
    }
    st.identity.delete_client(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}
