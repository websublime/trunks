//! `trunks create|update|delete user|mcp-client` (enterprise mode, admin).

use crate::out::outln;
use clap::{Args as ClapArgs, Subcommand};
use snafu::{ResultExt, Whatever};
use trunks_client::Client;
use trunks_types::api::{
    CreateMcpClientRequest, CreateUserRequest, UpdateMcpClientRequest, UpdateUserRequest,
};

#[derive(Debug, ClapArgs)]
pub struct CreateArgs {
    #[command(subcommand)]
    what: CreateWhat,
}

#[derive(Debug, Subcommand)]
enum CreateWhat {
    /// A tool group from a JSON file (`-c`)
    Group(super::groups::FileArgs),
    /// A team (enterprise mode)
    Team(super::access::TeamArgs),
    /// A human user of the REST API and the CLI (role `user`)
    User {
        /// Username (no whitespace)
        name: Option<String>,
        /// Custom token (≥ 8 chars, no whitespace); generated when omitted
        #[arg(long, allow_hyphen_values = true)]
        access_token: Option<String>,
        /// JSON file with `username` and optional `access_token` (`${VAR}` expanded)
        #[arg(short = 'c', long = "conf", value_name = "FILE")]
        conf: Option<std::path::PathBuf>,
    },
    /// A machine identity for the MCP endpoints
    McpClient {
        /// Client name (no whitespace)
        name: Option<String>,
        /// Server the client may reach (repeatable, or comma-separated); `*` for all
        #[arg(long = "allow", value_name = "SERVER", value_delimiter = ',')]
        allow: Vec<String>,
        #[arg(long)]
        description: Option<String>,
        /// Custom token (≥ 8 chars, no whitespace); generated when omitted
        #[arg(long, allow_hyphen_values = true)]
        access_token: Option<String>,
        /// Owner: `user:<name>` or `team:<name>` (admins may omit; users default to themselves)
        #[arg(long, value_name = "KIND:NAME")]
        owner: Option<String>,
        /// JSON file with `name`, `description`, `allow_list`, `access_token`, `owner` (`${VAR}` expanded)
        #[arg(short = 'c', long = "conf", value_name = "FILE")]
        conf: Option<std::path::PathBuf>,
    },
}

fn parse_owner(s: &str) -> Result<trunks_types::api::Owner, Whatever> {
    let Some((kind, name)) = s.split_once(':') else {
        snafu::whatever!("--owner must be user:<name> or team:<name>");
    };
    if kind != "user" && kind != "team" {
        snafu::whatever!("--owner must be user:<name> or team:<name>");
    }
    Ok(trunks_types::api::Owner { kind: kind.into(), name: name.into() })
}

fn read_conf<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> Result<T, Whatever> {
    let text =
        std::fs::read_to_string(path).whatever_context(format!("reading {}", path.display()))?;
    let mut value: serde_json::Value = serde_json::from_str(&text)
        .whatever_context(format!("{} is not valid JSON", path.display()))?;
    crate::envsubst::expand(&mut value)?;
    serde_json::from_value(value).whatever_context(format!("{} is not valid", path.display()))
}

fn allow_list_notes(allow_list: &[String], owned: bool) {
    let wildcard = allow_list.iter().any(|a| a == "*");
    match (wildcard, allow_list.is_empty(), owned) {
        (true, _, true) | (_, true, true) => {
            outln!("Note: on an owned client this means everything its owner can access.");
        }
        (true, _, false) => outln!(
            "Warning: this client can access every registered server (allow list contains \"*\")."
        ),
        (_, true, false) => outln!(
            "Note: the allow list is empty; this client cannot reach any server until it is updated."
        ),
        _ => {}
    }
}

pub async fn create(client: &Client, registry: &str, args: CreateArgs) -> Result<(), Whatever> {
    match args.what {
        CreateWhat::Group(a) => return super::groups::create(client, registry, a).await,
        CreateWhat::Team(a) => return super::access::create_team(client, a).await,
        CreateWhat::User { name, access_token, conf } => {
            let mut req = match &conf {
                Some(p) => read_conf::<CreateUserRequest>(p)?,
                None => CreateUserRequest::default(),
            };
            if let Some(n) = name {
                req.username = n;
            }
            if access_token.is_some() {
                req.access_token = access_token;
            }
            let u = client.create_user(&req).await.whatever_context("creating user")?;
            outln!("User {} created.", u.username);
            outln!("Access token: {}", u.access_token);
            outln!("Log in with: trunks login {}", u.access_token);
            outln!("This token is shown only once.");
        }
        CreateWhat::McpClient { name, allow, description, access_token, owner, conf } => {
            let mut req = match &conf {
                Some(p) => read_conf::<CreateMcpClientRequest>(p)?,
                None => CreateMcpClientRequest::default(),
            };
            if let Some(n) = name {
                req.name = n;
            }
            if !allow.is_empty() {
                req.allow_list = allow;
            }
            if let Some(d) = description {
                req.description = d;
            }
            if access_token.is_some() {
                req.access_token = access_token;
            }
            if let Some(o) = &owner {
                req.owner = Some(parse_owner(o)?);
            }
            let c = client.create_client(&req).await.whatever_context("creating MCP client")?;
            outln!("MCP client {} created.", c.name);
            if let Some(o) = &c.owner {
                outln!("Owner: {} {}", o.kind, o.name);
            }
            outln!("Access token: {}", c.access_token.unwrap_or_default());
            outln!("Allowed servers: {}", list_or_none(&c.allow_list));
            allow_list_notes(&c.allow_list, c.owner.is_some());
            outln!("This token is shown only once.");
        }
    }
    Ok(())
}

fn list_or_none(l: &[String]) -> String {
    if l.is_empty() { "(none)".into() } else { l.join(", ") }
}

#[derive(Debug, ClapArgs)]
pub struct UpdateArgs {
    #[command(subcommand)]
    what: UpdateWhat,
}

#[derive(Debug, Subcommand)]
enum UpdateWhat {
    /// Replace a tool group's definition from a JSON file (`-c`)
    Group(super::groups::UpdateFileArgs),
    /// Rotate a user's token
    User {
        name: String,
        /// Custom token; generated when omitted
        #[arg(long, allow_hyphen_values = true)]
        access_token: Option<String>,
    },
    /// Change an MCP client's allow list, description or token
    McpClient {
        name: String,
        /// New allow list (repeatable or comma-separated); replaces the current one
        #[arg(long = "allow", value_name = "SERVER", value_delimiter = ',')]
        allow: Option<Vec<String>>,
        #[arg(long)]
        description: Option<String>,
        /// Set a custom token
        #[arg(long, conflicts_with = "rotate_token", allow_hyphen_values = true)]
        access_token: Option<String>,
        /// Generate a new token
        #[arg(long)]
        rotate_token: bool,
    },
}

pub async fn update(client: &Client, args: UpdateArgs) -> Result<(), Whatever> {
    match args.what {
        UpdateWhat::Group(a) => return super::groups::update(client, a).await,
        UpdateWhat::User { name, access_token } => {
            let u = client
                .update_user(&name, &UpdateUserRequest { access_token })
                .await
                .whatever_context("updating user")?;
            outln!("Token of user {} rotated.", u.username);
            outln!("Access token: {}", u.access_token);
            outln!("This token is shown only once.");
        }
        UpdateWhat::McpClient { name, allow, description, access_token, rotate_token } => {
            if allow.is_none() && description.is_none() && access_token.is_none() && !rotate_token {
                snafu::whatever!(
                    "nothing to update: pass --allow, --description, --access-token or --rotate-token"
                );
            }
            let req = UpdateMcpClientRequest {
                description,
                allow_list: allow,
                access_token,
                rotate_token,
            };
            let c =
                client.update_client(&name, &req).await.whatever_context("updating MCP client")?;
            outln!("MCP client {} updated.", c.name);
            outln!("Allowed servers: {}", list_or_none(&c.allow_list));
            allow_list_notes(&c.allow_list, c.owner.is_some());
            if let Some(t) = c.access_token {
                outln!("Access token: {t}");
                outln!("This token is shown only once.");
            }
        }
    }
    Ok(())
}

#[derive(Debug, ClapArgs)]
pub struct DeleteArgs {
    #[command(subcommand)]
    what: DeleteWhat,
}

#[derive(Debug, Subcommand)]
enum DeleteWhat {
    Group(super::groups::NameArgs),
    Team(super::access::TeamNameArgs),
    User { name: String },
    McpClient { name: String },
}

pub async fn delete(client: &Client, args: DeleteArgs) -> Result<(), Whatever> {
    match args.what {
        DeleteWhat::Group(a) => return super::groups::delete(client, a).await,
        DeleteWhat::Team(a) => return super::access::delete_team(client, a).await,
        DeleteWhat::User { name } => {
            client.delete_user(&name).await.whatever_context("deleting user")?;
            outln!("User {name} deleted.");
        }
        DeleteWhat::McpClient { name } => {
            client.delete_client(&name).await.whatever_context("deleting MCP client")?;
            outln!("MCP client {name} deleted.");
        }
    }
    Ok(())
}
