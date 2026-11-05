//! `/api/v0/teams*` and `/api/v0/grants` (D14). Enterprise mode. Teams are readable by any
//! user; team membership is managed by admins or the team's maintainers; grants are admin-only.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Extension, Json, Router};
use trunks_core::Error;
use trunks_core::access::{SubjectKind, TargetKind, TeamRole};
use trunks_core::identity::UserPrincipal;
use trunks_core::ports::{GrantRecord, TeamRecord};
use trunks_types::api::{
    AddTeamMemberRequest, GrantSpec, GrantsQuery, Team, TeamDetail, TeamMember,
};
use trunks_types::error::messages;

use crate::error::{ApiError, JsonBody};
use crate::state::AppState;

/// Readable by every user; mutations check the caller inside.
pub fn teams_router() -> Router<AppState> {
    Router::new()
        .route("/teams", get(list_teams).post(create_team))
        .route("/teams/{name}", get(get_team).put(update_team).delete(delete_team))
        .route("/teams/{name}/members", axum::routing::post(add_member))
        .route("/teams/{name}/members/{username}", axum::routing::delete(remove_member))
}

/// Admin only.
pub fn grants_router() -> Router<AppState> {
    Router::new()
        .route("/grants", get(list_grants).post(add_grant).delete(remove_grant))
        .route("/servers/{name}/access", get(server_access))
        .route("/tool-groups/{name}/access", get(group_access))
}

fn require_admin(p: &UserPrincipal) -> Result<(), ApiError> {
    if p.is_admin() {
        Ok(())
    } else {
        Err(ApiError(Error::Forbidden { reason: messages::USER_NOT_AUTHORIZED.into() }))
    }
}

fn team_view(t: &TeamRecord) -> Team {
    Team { name: t.name.clone(), description: t.description.clone() }
}

fn grant_view(g: &GrantRecord) -> GrantSpec {
    GrantSpec {
        subject_kind: g.subject_kind.as_str().to_owned(),
        subject: g.subject_name.clone(),
        target_kind: g.target_kind.as_str().to_owned(),
        target: g.target_name.clone(),
    }
}

/// `(subject, target)` of a wire grant.
type ParsedGrant<'a> = ((SubjectKind, &'a str), (TargetKind, &'a str));

fn parse_grant(g: &GrantSpec) -> Result<ParsedGrant<'_>, ApiError> {
    let sk = SubjectKind::parse(&g.subject_kind)
        .ok_or_else(|| Error::invalid_input(messages::INVALID_SUBJECT_KIND))?;
    let tk = TargetKind::parse(&g.target_kind)
        .ok_or_else(|| Error::invalid_input(messages::INVALID_TARGET_KIND))?;
    Ok(((sk, g.subject.as_str()), (tk, g.target.as_str())))
}

async fn list_teams(State(st): State<AppState>) -> Result<Json<Vec<Team>>, ApiError> {
    Ok(Json(st.identity.teams().await?.iter().map(team_view).collect()))
}

async fn detail(st: &AppState, t: &TeamRecord) -> Result<TeamDetail, ApiError> {
    let members = st
        .identity
        .members(t)
        .await?
        .into_iter()
        .map(|m| TeamMember { username: m.username, role: m.role.as_str().to_owned() })
        .collect();
    Ok(TeamDetail { team: team_view(t), members })
}

async fn get_team(
    State(st): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<TeamDetail>, ApiError> {
    let t = st.identity.team_by_name(&name).await?;
    Ok(Json(detail(&st, &t).await?))
}

async fn create_team(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
    JsonBody(body): JsonBody<Team>,
) -> Result<(StatusCode, Json<TeamDetail>), ApiError> {
    require_admin(&p)?;
    let t = st.identity.create_team(&body.name, &body.description).await?;
    Ok((StatusCode::CREATED, Json(detail(&st, &t).await?)))
}

async fn update_team(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
    Path(name): Path<String>,
    JsonBody(body): JsonBody<Team>,
) -> Result<Json<TeamDetail>, ApiError> {
    require_admin(&p)?;
    let t = st.identity.update_team(&name, &body.description).await?;
    Ok(Json(detail(&st, &t).await?))
}

async fn delete_team(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
    Path(name): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_admin(&p)?;
    st.identity.delete_team(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Admin or a maintainer of the team.
async fn manageable_team(
    st: &AppState,
    p: &UserPrincipal,
    name: &str,
) -> Result<TeamRecord, ApiError> {
    let t = st.identity.team_by_name(name).await?;
    if p.may_act_for((SubjectKind::Team, t.id)) {
        Ok(t)
    } else {
        Err(ApiError(Error::Forbidden { reason: messages::USER_NOT_AUTHORIZED.into() }))
    }
}

async fn add_member(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
    Path(name): Path<String>,
    JsonBody(body): JsonBody<AddTeamMemberRequest>,
) -> Result<Json<TeamDetail>, ApiError> {
    let t = manageable_team(&st, &p, &name).await?;
    let role = match body.role.as_deref() {
        None | Some("") => TeamRole::Member,
        Some(r) => {
            TeamRole::parse(r).ok_or_else(|| Error::invalid_input(messages::INVALID_TEAM_ROLE))?
        }
    };
    st.identity.add_member(&t, &body.username, role).await?;
    Ok(Json(detail(&st, &t).await?))
}

async fn remove_member(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
    Path((name, username)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let t = manageable_team(&st, &p, &name).await?;
    st.identity.remove_member(&t, &username).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_grants(
    State(st): State<AppState>,
    Query(q): Query<GrantsQuery>,
) -> Result<Json<Vec<GrantSpec>>, ApiError> {
    let target = match (q.target_kind.as_deref(), q.target.as_deref()) {
        (Some(k), Some(t)) => Some((
            TargetKind::parse(k)
                .ok_or_else(|| Error::invalid_input(messages::INVALID_TARGET_KIND))?,
            t,
        )),
        _ => None,
    };
    Ok(Json(st.identity.grants(target).await?.iter().map(grant_view).collect()))
}

async fn add_grant(
    State(st): State<AppState>,
    JsonBody(body): JsonBody<GrantSpec>,
) -> Result<(StatusCode, Json<GrantSpec>), ApiError> {
    let (subject, target) = parse_grant(&body)?;
    let created = st.identity.grant(subject, target).await?;
    Ok((if created { StatusCode::CREATED } else { StatusCode::OK }, Json(body)))
}

async fn remove_grant(
    State(st): State<AppState>,
    JsonBody(body): JsonBody<GrantSpec>,
) -> Result<StatusCode, ApiError> {
    let (subject, target) = parse_grant(&body)?;
    st.identity.revoke(subject, target).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn server_access(
    State(st): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<Vec<GrantSpec>>, ApiError> {
    Ok(Json(
        st.identity
            .grants(Some((TargetKind::Server, &name)))
            .await?
            .iter()
            .map(grant_view)
            .collect(),
    ))
}

async fn group_access(
    State(st): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<Vec<GrantSpec>>, ApiError> {
    Ok(Json(
        st.identity
            .grants(Some((TargetKind::ToolGroup, &name)))
            .await?
            .iter()
            .map(grant_view)
            .collect(),
    ))
}
