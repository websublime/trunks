//! `trunks create|get|update|delete group` and `list groups`.

use crate::out::outln;
use clap::Args as ClapArgs;
use snafu::{ResultExt, Whatever};
use trunks_client::Client;
use trunks_types::api::ToolGroup;

#[derive(Debug, ClapArgs)]
pub struct FileArgs {
    /// JSON file: `{name, description, included_tools[], included_servers[], excluded_tools[]}`
    #[arg(short = 'c', long = "conf", value_name = "FILE")]
    conf: std::path::PathBuf,
}

#[derive(Debug, ClapArgs)]
pub struct UpdateFileArgs {
    /// Group name (the file's `name` is ignored)
    name: String,
    #[arg(short = 'c', long = "conf", value_name = "FILE")]
    conf: std::path::PathBuf,
}

#[derive(Debug, ClapArgs)]
pub struct NameArgs {
    name: String,
}

fn read_group(path: &std::path::Path) -> Result<ToolGroup, Whatever> {
    let text =
        std::fs::read_to_string(path).whatever_context(format!("reading {}", path.display()))?;
    let mut value: serde_json::Value = serde_json::from_str(&text)
        .whatever_context(format!("{} is not valid JSON", path.display()))?;
    crate::envsubst::expand(&mut value)?;
    serde_json::from_value(value)
        .whatever_context(format!("{} is not a valid tool group", path.display()))
}

fn print_group(g: &ToolGroup, endpoint: Option<&str>, registry: &str) {
    outln!("{}", g.name);
    if !g.description.is_empty() {
        outln!("  {}", g.description);
    }
    outln!("  included tools:   {}", join(&g.included_tools));
    outln!("  included servers: {}", join(&g.included_servers));
    outln!("  excluded tools:   {}", join(&g.excluded_tools));
    if let Some(e) = endpoint {
        outln!("  MCP endpoint:     {}{e}", registry.trim_end_matches('/'));
    }
}

fn join(l: &[String]) -> String {
    if l.is_empty() { "(none)".into() } else { l.join(", ") }
}

pub async fn create(client: &Client, registry: &str, args: FileArgs) -> Result<(), Whatever> {
    let group = read_group(&args.conf)?;
    let created = client.create_group(&group).await.whatever_context("creating tool group")?;
    outln!("Tool group {} created.", created.group.name);
    print_group(&created.group, Some(&created.streamable_http_endpoint), registry);
    let tools = client.group_tools(&created.group.name).await.whatever_context("listing tools")?;
    outln!("  effective tools ({}):", tools.len());
    for t in &tools {
        outln!("    {}", t.name);
    }
    Ok(())
}

pub async fn get(client: &Client, registry: &str, args: NameArgs) -> Result<(), Whatever> {
    let g = client.get_group(&args.name).await.whatever_context("fetching tool group")?;
    print_group(&g.group, Some(&g.streamable_http_endpoint), registry);
    let tools = client.group_tools(&args.name).await.whatever_context("listing tools")?;
    outln!("  effective tools ({}):", tools.len());
    for t in &tools {
        outln!("    {}", t.name);
    }
    Ok(())
}

pub async fn list(client: &Client) -> Result<(), Whatever> {
    let groups = client.list_groups().await.whatever_context("listing tool groups")?;
    if groups.is_empty() {
        outln!("No tool groups.");
        return Ok(());
    }
    for g in &groups {
        outln!(
            "{}  tools: {}  servers: {}  excluded: {}",
            g.name,
            g.included_tools.len(),
            g.included_servers.len(),
            g.excluded_tools.len()
        );
    }
    Ok(())
}

pub async fn update(client: &Client, args: UpdateFileArgs) -> Result<(), Whatever> {
    let group = read_group(&args.conf)?;
    let r =
        client.update_group(&args.name, &group).await.whatever_context("updating tool group")?;
    let d = trunks_core::groups::diff(&r.old, &r.new);
    if d.is_empty() && r.old.description == r.new.description {
        outln!("No changes detected for tool group {}.", r.name);
        return Ok(());
    }
    outln!("Tool group {} updated.", r.name);
    for (label, items) in [
        ("tools added", &d.added_tools),
        ("tools removed", &d.removed_tools),
        ("servers added", &d.added_servers),
        ("servers removed", &d.removed_servers),
        ("exclusions added", &d.added_exclusions),
        ("exclusions removed", &d.removed_exclusions),
    ] {
        if !items.is_empty() {
            outln!("  {label}: {}", items.join(", "));
        }
    }
    if r.old.description != r.new.description {
        outln!("  description: {:?} -> {:?}", r.old.description, r.new.description);
    }
    Ok(())
}

pub async fn delete(client: &Client, args: NameArgs) -> Result<(), Whatever> {
    client.delete_group(&args.name).await.whatever_context("deleting tool group")?;
    outln!("Tool group {} deleted.", args.name);
    Ok(())
}
