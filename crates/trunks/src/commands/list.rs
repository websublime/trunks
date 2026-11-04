//! `trunks list servers|tools|prompts|resources`

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
        #[arg(long)]
        server: Option<String>,
    },
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
                println!("No servers registered.");
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
            println!("{t}");
        }
        What::Tools { server } => {
            let tools =
                client.list_tools(server.as_deref()).await.whatever_context("listing tools")?;
            if tools.is_empty() {
                println!("No tools found.");
                return Ok(());
            }
            let mut t = table(&["name", "status", "description"]);
            for x in tools {
                t.add_row([x.name.to_string(), flag(x.enabled).into(), x.description]);
            }
            println!("{t}");
        }
        What::Prompts { server } => {
            let prompts =
                client.list_prompts(server.as_deref()).await.whatever_context("listing prompts")?;
            if prompts.is_empty() {
                println!("No prompts found.");
                return Ok(());
            }
            let mut t = table(&["name", "status", "description"]);
            for x in prompts {
                t.add_row([x.name.to_string(), flag(x.enabled).into(), x.description]);
            }
            println!("{t}");
        }
        What::Resources { server } => {
            let resources = client
                .list_resources(server.as_deref())
                .await
                .whatever_context("listing resources")?;
            if resources.is_empty() {
                println!("No resources found.");
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
            println!("{t}");
        }
        What::Users => {
            let users = client.list_users().await.whatever_context("listing users")?;
            let mut t = table(&["username", "role"]);
            for u in users {
                t.add_row([u.username, u.role.to_string()]);
            }
            println!("{t}");
        }
        What::McpClients => {
            let clients = client.list_clients().await.whatever_context("listing MCP clients")?;
            if clients.is_empty() {
                println!("No MCP clients.");
                return Ok(());
            }
            let mut t = table(&["name", "allowed servers", "description"]);
            for c in clients {
                let allow =
                    if c.allow_list.is_empty() { "(none)".into() } else { c.allow_list.join(", ") };
                t.add_row([c.name, allow, c.description]);
            }
            println!("{t}");
        }
    }
    Ok(())
}
