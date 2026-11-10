//! `trunks deregister`, `trunks enable|disable`, `trunks export`, `trunks get`.

use clap::{Args as ClapArgs, Subcommand};
use snafu::{ResultExt, Whatever};
use trunks_client::Client;

#[derive(Debug, ClapArgs)]
pub struct DeregisterArgs {
    /// Server name
    name: String,
}

pub async fn deregister(client: &Client, args: DeregisterArgs) -> Result<(), Whatever> {
    client.deregister_server(&args.name).await.whatever_context("deregistering server")?;
    println!("Server {} deregistered successfully!", args.name);
    Ok(())
}

#[derive(Debug, ClapArgs)]
pub struct ToggleArgs {
    #[command(subcommand)]
    what: ToggleWhat,
}

#[derive(Debug, Subcommand)]
enum ToggleWhat {
    /// A registered server (its tools, prompts and resources follow)
    Server { name: String },
    /// A canonical tool name, or a server name for all its tools
    Tool { entity: String },
    /// A canonical prompt name, or a server name for all its prompts
    Prompt { entity: String },
    /// A trunks:// resource URI, or a server name for all its resources
    Resource { entity: String },
}

fn print_list(label: &str, items: &[String]) {
    if !items.is_empty() {
        println!("{label} ({}):", items.len());
        for i in items {
            println!("  {i}");
        }
    }
}

pub async fn toggle(client: &Client, args: ToggleArgs, enabled: bool) -> Result<(), Whatever> {
    let verb = if enabled { "enabled" } else { "disabled" };
    let ctx = if enabled { "enabling" } else { "disabling" };
    let (kind, entity) = match args.what {
        ToggleWhat::Server { name } => {
            let r = client.set_server_enabled(&name, enabled).await.whatever_context(ctx)?;
            println!("Server {name} {verb}.");
            print_list("Tools affected", &r.tools_affected);
            print_list("Prompts affected", &r.prompts_affected);
            print_list("Resources affected", &r.resources_affected);
            return Ok(());
        }
        ToggleWhat::Tool { entity } => ("tools", entity),
        ToggleWhat::Prompt { entity } => ("prompts", entity),
        ToggleWhat::Resource { entity } => ("resources", entity),
    };
    let r = client.set_entities_enabled(kind, &entity, enabled).await.whatever_context(ctx)?;
    let affected = match kind {
        "tools" => r.tools_affected,
        "prompts" => r.prompts_affected,
        _ => r.resources_affected,
    };
    if affected.is_empty() {
        println!("No {kind} changed state.");
    } else {
        print_list(&format!("{} {verb}", capitalize(kind)), &affected);
    }
    Ok(())
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[derive(Debug, ClapArgs)]
pub struct ExportArgs {
    /// Output directory (must not exist or be empty)
    #[arg(short = 'd', long = "dir", default_value = ".trunks")]
    dir: std::path::PathBuf,
}

/// Writes `servers/<name>.json` for every registered server, **including credentials**
/// (the files are the input format of `trunks register -c`).
pub async fn export(client: &Client, args: ExportArgs) -> Result<(), Whatever> {
    if args.dir.exists() {
        let mut entries = std::fs::read_dir(&args.dir)
            .whatever_context(format!("reading {}", args.dir.display()))?;
        if entries.next().is_some() {
            snafu::whatever!("directory {} is not empty", args.dir.display());
        }
    }
    let servers = client.server_configs().await.whatever_context("exporting server configs")?;
    let groups = client.list_groups().await.whatever_context("exporting tool groups")?;
    let dir = args.dir.join("servers");
    std::fs::create_dir_all(&dir).whatever_context(format!("creating {}", dir.display()))?;
    for s in &servers {
        let path = dir.join(format!("{}.json", s.name));
        let json = serde_json::to_string_pretty(s).whatever_context("serializing")?;
        std::fs::write(&path, json).whatever_context(format!("writing {}", path.display()))?;
        println!("wrote {}", path.display());
    }
    let gdir = args.dir.join("groups");
    std::fs::create_dir_all(&gdir).whatever_context(format!("creating {}", gdir.display()))?;
    for g in &groups {
        let path = gdir.join(format!("{}.json", g.name));
        let json = serde_json::to_string_pretty(g).whatever_context("serializing")?;
        std::fs::write(&path, json).whatever_context(format!("writing {}", path.display()))?;
        println!("wrote {}", path.display());
    }
    println!(
        "Exported {} server(s) and {} tool group(s) to {}",
        servers.len(),
        groups.len(),
        args.dir.display()
    );
    println!("Warning: the exported files contain credentials. Keep them private.");
    Ok(())
}

#[derive(Debug, ClapArgs)]
pub struct GetArgs {
    #[command(subcommand)]
    what: GetWhat,
}

#[derive(Debug, Subcommand)]
enum GetWhat {
    /// Show a tool group and its effective tools
    Group(super::groups::NameArgs),
    /// Render a prompt
    Prompt {
        /// Canonical prompt name (<server>__<prompt>)
        name: String,
        /// Prompt argument `key=value` (repeatable)
        #[arg(long = "arg", value_name = "KEY=VALUE")]
        args: Vec<String>,
    },
    /// Show a resource; `--read` fetches its contents from the upstream
    Resource {
        /// trunks:// resource URI
        uri: String,
        #[arg(long)]
        read: bool,
    },
}

pub async fn get(client: &Client, registry: &str, args: GetArgs) -> Result<(), Whatever> {
    match args.what {
        GetWhat::Group(a) => return super::groups::get(client, registry, a).await,
        GetWhat::Prompt { name, args } => {
            let mut map = serde_json::Map::new();
            for a in &args {
                let Some((k, v)) = a.split_once('=') else {
                    snafu::whatever!("expected KEY=VALUE, got '{a}'");
                };
                map.insert(k.trim().to_owned(), serde_json::Value::String(v.to_owned()));
            }
            let r = client.render_prompt(&name, map).await.whatever_context("rendering prompt")?;
            if let Some(d) = &r.description {
                println!("{d}\n");
            }
            for m in &r.messages {
                let role = m["role"].as_str().unwrap_or("?");
                let text = m["content"]["text"].as_str().map_or_else(
                    || serde_json::to_string_pretty(&m["content"]).unwrap_or_default(),
                    str::to_owned,
                );
                println!("[{role}]\n{text}\n");
            }
        }
        GetWhat::Resource { uri, read } => {
            if read {
                let r = client.read_resource(&uri).await.whatever_context("reading resource")?;
                for c in &r.contents {
                    if let Some(t) = c["text"].as_str() {
                        println!("{t}");
                    } else if let Some(b) = c["blob"].as_str() {
                        let path = crate::commands::invoke::save_blob(
                            "resource",
                            c["mimeType"].as_str(),
                            b,
                        )?;
                        println!("Saved binary contents to {}", path.display());
                    }
                }
            } else {
                let all = client.list_resources(None).await.whatever_context("listing")?;
                let Some(r) = all.iter().find(|r| r.uri.to_string() == uri) else {
                    snafu::whatever!("resource '{uri}' not found");
                };
                println!("{}\n  server: {}\n  name: {}", r.uri, r.server, r.name);
                if let Some(m) = &r.mime_type {
                    println!("  mime type: {m}");
                }
                if !r.description.is_empty() {
                    println!("  {}", r.description);
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug, ClapArgs)]
pub struct OAuthCompleteArgs {
    /// Session id printed by `trunks register`
    session_id: String,
    #[arg(long, allow_hyphen_values = true)]
    code: String,
    #[arg(long, allow_hyphen_values = true)]
    state: String,
}

/// Completes a pending OAuth authorization started with a custom redirect URI.
pub async fn oauth_complete(client: &Client, args: OAuthCompleteArgs) -> Result<(), Whatever> {
    let r = client
        .complete_oauth(&args.session_id, &args.code, &args.state)
        .await
        .whatever_context("completing the OAuth authorization")?;
    match r.server {
        Some(s) => println!("Server {} registered successfully!", s.name),
        None => snafu::whatever!("the server did not return the registered server"),
    }
    Ok(())
}
