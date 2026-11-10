//! `trunks create|delete team`, `add|remove team-member`, `grant|revoke`, `list teams|grants`.

use crate::out::outln;
use clap::{Args as ClapArgs, ValueEnum};
use snafu::{ResultExt, Whatever};
use trunks_client::Client;
use trunks_types::api::{AddTeamMemberRequest, GrantSpec, Team};

#[derive(Debug, ClapArgs)]
pub struct TeamArgs {
    /// Team name (no whitespace)
    pub name: String,
    #[arg(long, default_value = "")]
    pub description: String,
}

#[derive(Debug, ClapArgs)]
pub struct TeamNameArgs {
    pub name: String,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum RoleArg {
    Member,
    Maintainer,
}

#[derive(Debug, ClapArgs)]
pub struct MemberArgs {
    /// Team name
    pub team: String,
    /// Username
    pub user: String,
    #[arg(long, value_enum, default_value = "member")]
    pub role: RoleArg,
}

#[derive(Debug, ClapArgs)]
pub struct RemoveMemberArgs {
    pub team: String,
    pub user: String,
}

/// `--user X | --team X` and `--server Y | --group Y` (`--server '*'` = every server).
#[derive(Debug, ClapArgs)]
pub struct GrantArgs {
    #[arg(long, conflicts_with = "team")]
    pub user: Option<String>,
    #[arg(long)]
    pub team: Option<String>,
    #[arg(long, conflicts_with = "group")]
    pub server: Option<String>,
    #[arg(long)]
    pub group: Option<String>,
}

#[derive(Debug, ClapArgs)]
pub struct ListGrantsArgs {
    /// Only grants on this server
    #[arg(long, conflicts_with = "group")]
    pub server: Option<String>,
    /// Only grants on this tool group
    #[arg(long)]
    pub group: Option<String>,
}

fn spec(args: &GrantArgs) -> Result<GrantSpec, Whatever> {
    let (subject_kind, subject) = match (&args.user, &args.team) {
        (Some(u), None) => ("user", u.clone()),
        (None, Some(t)) => ("team", t.clone()),
        _ => snafu::whatever!("pass exactly one of --user or --team"),
    };
    let (target_kind, target) = match (&args.server, &args.group) {
        (Some(s), None) => ("server", s.clone()),
        (None, Some(g)) => ("tool_group", g.clone()),
        _ => snafu::whatever!("pass exactly one of --server or --group"),
    };
    Ok(GrantSpec {
        subject_kind: subject_kind.into(),
        subject,
        target_kind: target_kind.into(),
        target,
    })
}

fn describe(g: &GrantSpec) -> String {
    format!("{} {} → {} {}", g.subject_kind, g.subject, g.target_kind.replace('_', " "), g.target)
}

pub async fn create_team(client: &Client, args: TeamArgs) -> Result<(), Whatever> {
    let t = client
        .create_team(&Team { name: args.name, description: args.description })
        .await
        .whatever_context("creating team")?;
    outln!("Team {} created.", t.team.name);
    Ok(())
}

pub async fn delete_team(client: &Client, args: TeamNameArgs) -> Result<(), Whatever> {
    client.delete_team(&args.name).await.whatever_context("deleting team")?;
    outln!("Team {} deleted.", args.name);
    Ok(())
}

pub async fn list_teams(client: &Client) -> Result<(), Whatever> {
    let teams = client.list_teams().await.whatever_context("listing teams")?;
    if teams.is_empty() {
        outln!("No teams.");
        return Ok(());
    }
    for t in teams {
        let d = client.get_team(&t.name).await.whatever_context("fetching team")?;
        let members: Vec<String> =
            d.members.iter().map(|m| format!("{} ({})", m.username, m.role)).collect();
        outln!(
            "{}  {}\n  members: {}",
            t.name,
            t.description,
            if members.is_empty() { "(none)".into() } else { members.join(", ") }
        );
    }
    Ok(())
}

pub async fn add_member(client: &Client, args: MemberArgs) -> Result<(), Whatever> {
    let role = match args.role {
        RoleArg::Member => "member",
        RoleArg::Maintainer => "maintainer",
    };
    let d = client
        .add_team_member(
            &args.team,
            &AddTeamMemberRequest { username: args.user.clone(), role: Some(role.into()) },
        )
        .await
        .whatever_context("adding team member")?;
    outln!("{} is now a {role} of team {}.", args.user, d.team.name);
    Ok(())
}

pub async fn remove_member(client: &Client, args: RemoveMemberArgs) -> Result<(), Whatever> {
    client.remove_team_member(&args.team, &args.user).await.whatever_context("removing member")?;
    outln!("{} removed from team {}.", args.user, args.team);
    Ok(())
}

pub async fn grant(client: &Client, args: GrantArgs) -> Result<(), Whatever> {
    let g = spec(&args)?;
    client.grant(&g).await.whatever_context("granting access")?;
    outln!("Granted: {}", describe(&g));
    Ok(())
}

pub async fn revoke(client: &Client, args: GrantArgs) -> Result<(), Whatever> {
    let g = spec(&args)?;
    client.revoke(&g).await.whatever_context("revoking access")?;
    outln!("Revoked: {}", describe(&g));
    Ok(())
}

pub async fn list_grants(client: &Client, args: ListGrantsArgs) -> Result<(), Whatever> {
    let target = match (&args.server, &args.group) {
        (Some(s), _) => Some(("server", s.as_str())),
        (_, Some(g)) => Some(("tool_group", g.as_str())),
        _ => None,
    };
    let grants = client.list_grants(target).await.whatever_context("listing grants")?;
    if grants.is_empty() {
        outln!("No grants.");
        return Ok(());
    }
    for g in &grants {
        outln!("{}", describe(g));
    }
    Ok(())
}
