//! `trunks usage <tool>` — shows a tool's input schema in a readable form.

use crate::out::outln;
use clap::Args as ClapArgs;
use snafu::{ResultExt, Whatever};
use trunks_client::Client;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Canonical tool name (<server>__<tool>)
    name: String,
}

pub async fn run(client: &Client, args: Args) -> Result<(), Whatever> {
    let tool = client.get_tool(&args.name).await.whatever_context("fetching tool")?;
    outln!("{}", tool.name);
    if !tool.description.is_empty() {
        outln!("  {}", tool.description);
    }
    outln!(
        "  server: {}   status: {}",
        tool.server,
        if tool.enabled { "enabled" } else { "disabled" }
    );
    let Some(schema) = tool.input_schema else {
        outln!("\nNo input schema.");
        return Ok(());
    };
    let required: Vec<&str> = schema["required"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    match schema["properties"].as_object() {
        Some(props) if !props.is_empty() => {
            outln!("\nInput parameters:");
            let mut names: Vec<&String> = props.keys().collect();
            names.sort();
            for name in names {
                let p = &props[name];
                let kind = p["type"].as_str().unwrap_or("any");
                let req = if required.contains(&name.as_str()) { "required" } else { "optional" };
                outln!("  - {name} ({kind}, {req})");
                if let Some(d) = p["description"].as_str() {
                    outln!("      {d}");
                }
                if p.get("enum").is_some()
                    || p.get("items").is_some()
                    || p.get("properties").is_some()
                {
                    let detail = serde_json::to_string_pretty(p).unwrap_or_default();
                    for line in detail.lines() {
                        outln!("      {line}");
                    }
                }
            }
        }
        _ => outln!("\nThis tool takes no input parameters."),
    }
    if let Some(a) = tool.annotations.filter(|a| a.as_object().is_some_and(|o| !o.is_empty())) {
        outln!("\nAnnotations:");
        for (k, v) in a.as_object().into_iter().flatten() {
            outln!("  - {k} = {v}");
        }
    }
    Ok(())
}
