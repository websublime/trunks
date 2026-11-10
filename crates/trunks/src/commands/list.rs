//! `trunks list servers|tools|prompts|resources`

use crate::out::outln;
use clap::{Args as ClapArgs, Subcommand};
use comfy_table::{ContentArrangement, Table, presets::UTF8_FULL_CONDENSED};
use snafu::{ResultExt, Whatever};
use trunks_client::Client;

#[derive(Debug, ClapArgs)]
pub struct Args {
    #[command(subcommand)]
    what: What,
}

#[derive(Debug, Subcommand)]
enum What {
    /// Registered upstream servers
    Servers,
    /// Tools exposed by the gateway
    Tools {
        /// Only tools of this server
        #[arg(long, conflicts_with = "group")]
        server: Option<String>,
        /// Only the effective tools of this tool group
        #[arg(long)]
        group: Option<String>,
    },
    /// Tool groups
    Groups,
    /// Prompts exposed by the gateway
    Prompts {
        #[arg(long)]
        server: Option<String>,
    },
    /// Resources exposed by the gateway
    Resources {
        #[arg(long)]
        server: Option<String>,
    },
    /// Users of the REST API (enterprise mode)
    Users,
    /// MCP client identities (enterprise mode)
    McpClients,
    /// Teams (enterprise mode)
    Teams,
    /// Access grants (enterprise mode)
    Grants(super::access::ListGrantsArgs),
}

fn table(header: &[&str]) -> Table {
    let mut t = Table::new();
    t.load_preset(UTF8_FULL_CONDENSED)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(header);
    t
}

fn flag(enabled: bool) -> &'static str {
    if enabled { "enabled" } else { "disabled" }
}

pub async fn run(client: &Client, args: Args) -> Result<(), Whatever> {
    match args.what {
        What::Servers => {
            let servers = client.list_servers().await.whatever_context("listing servers")?;
            if servers.is_empty() {
                outln!("No servers registered.");
                return Ok(());
            }
            let mut t = table(&["name", "transport", "status", "session", "target", "description"]);
            for s in servers {
                let target = s
                    .url
                    .clone()
                    .or_else(|| s.command.as_ref().map(|c| format!("{c} {}", s.args.join(" "))))
                    .unwrap_or_default();
                t.add_row([
                    s.name.to_string(),
                    s.transport.to_string(),
                    flag(s.enabled).into(),
                    s.session_mode.to_string(),
                    target,
                    s.description,
                ]);
            }
            outln!("{t}");
        }
        What::Groups => return super::groups::list(client).await,
        What::Tools { server, group } => {
            let tools = match &group {
                Some(g) => client.group_tools(g).await.whatever_context("listing group tools")?,
                None => {
                    client.list_tools(server.as_deref()).await.whatever_context("listing tools")?
                }
            };
            if tools.is_empty() {
                outln!("No tools found.");
                return Ok(());
            }
            let mut t = table(&["name", "status", "description"]);
            for x in tools {
                t.add_row([x.name.to_string(), flag(x.enabled).into(), x.description]);
            }
            outln!("{t}");
        }
        What::Prompts { server } => {
            let prompts =
                client.list_prompts(server.as_deref()).await.whatever_context("listing prompts")?;
            if prompts.is_empty() {
                outln!("No prompts found.");
                return Ok(());
            }
            let mut t = table(&["name", "status", "description"]);
            for x in prompts {
                t.add_row([x.name.to_string(), flag(x.enabled).into(), x.description]);
            }
            outln!("{t}");
        }
        What::Resources { server } => {
            let resources = client
                .list_resources(server.as_deref())
                .await
                .whatever_context("listing resources")?;
            if resources.is_empty() {
                outln!("No resources found.");
                return Ok(());
            }
            let mut t = table(&["uri", "name", "status", "mime", "description"]);
            for x in resources {
                t.add_row([
                    x.uri.to_string(),
                    x.name,
                    flag(x.enabled).into(),
                    x.mime_type.unwrap_or_default(),
                    x.description,
                ]);
            }
            outln!("{t}");
        }
        What::Users => list_users(client).await?,
        What::McpClients => list_clients(client).await?,
        What::Teams => return super::access::list_teams(client).await,
        What::Grants(a) => return super::access::list_grants(client, a).await,
    }
    Ok(())
}

async fn list_users(client: &Client) -> Result<(), Whatever> {
    let users = client.list_users().await.whatever_context("listing users")?;
    let mut t = table(&["username", "role"]);
    for u in users {
        t.add_row([u.username, u.role.to_string()]);
    }
    outln!("{t}");
    Ok(())
}

async fn list_clients(client: &Client) -> Result<(), Whatever> {
    let clients = client.list_clients().await.whatever_context("listing MCP clients")?;
    if clients.is_empty() {
        outln!("No MCP clients.");
        return Ok(());
    }
    let mut t = table(&["name", "owner", "allowed servers", "description"]);
    for c in clients {
        let allow = if c.allow_list.is_empty() {
            if c.owner.is_some() { "(owner's access)".into() } else { "(none)".into() }
        } else {
            c.allow_list.join(", ")
        };
        let owner = c.owner.map(|o| format!("{}:{}", o.kind, o.name)).unwrap_or_default();
        t.add_row([c.name, owner, allow, c.description]);
    }
    outln!("{t}");
    Ok(())
}
