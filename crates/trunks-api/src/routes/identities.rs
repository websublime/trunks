//! `/api/v0/users*` and `/api/v0/clients*` — enterprise mode, admin only.
//!
//! Tokens are serialized exactly twice: on creation and on rotation. Listings never carry them.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use trunks_types::api::{
    CreateMcpClientRequest, CreateUserRequest, McpClient, McpClientWithToken,
    UpdateMcpClientRequest, UpdateUserRequest, User, UserWithToken,
};

use trunks_core::domain::{McpClientRecord, UserRecord};

use crate::error::{ApiError, JsonBody};
use crate::state::AppState;

/// Routes behind `require_enterprise` + `require_admin`.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/users", get(list_users).post(create_user))
        .route("/users/{username}", axum::routing::put(update_user).delete(delete_user))
        .route("/clients", get(list_clients).post(create_client))
        .route("/clients/{name}", axum::routing::put(update_client).delete(delete_client))
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

async fn list_clients(State(st): State<AppState>) -> Result<Json<Vec<McpClient>>, ApiError> {
    Ok(Json(st.identity.clients().await?.iter().map(McpClientRecord::to_api).collect()))
}

async fn create_client(
    State(st): State<AppState>,
    JsonBody(body): JsonBody<CreateMcpClientRequest>,
) -> Result<(StatusCode, Json<McpClientWithToken>), ApiError> {
    let (client, token) = st
        .identity
        .create_client(
            &body.name,
            &body.description,
            &body.allow_list,
            body.access_token.as_deref(),
        )
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(McpClientWithToken {
            name: client.name,
            description: client.description,
            allow_list: client.allow_list,
            access_token: Some(token),
        }),
    ))
}

async fn update_client(
    State(st): State<AppState>,
    Path(name): Path<String>,
    JsonBody(body): JsonBody<UpdateMcpClientRequest>,
) -> Result<Json<McpClientWithToken>, ApiError> {
    let new_token = match (&body.access_token, body.rotate_token) {
        (Some(t), _) => Some(Some(t.as_str())),
        (None, true) => Some(None),
        (None, false) => None,
    };
    let (client, token) = st
        .identity
        .update_client(&name, body.description.as_deref(), body.allow_list.as_deref(), new_token)
        .await?;
    Ok(Json(McpClientWithToken {
        name: client.name,
        description: client.description,
        allow_list: client.allow_list,
        access_token: token,
    }))
}

async fn delete_client(
    State(st): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, ApiError> {
    st.identity.delete_client(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}
