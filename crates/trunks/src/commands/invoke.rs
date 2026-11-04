//! `trunks invoke <tool> --input '{...}'`

use clap::Args as ClapArgs;
use snafu::{ResultExt, Whatever};
use trunks_client::Client;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Canonical tool name (<server>__<tool>)
    name: String,
    /// JSON object with the tool's arguments
    #[arg(long, default_value = "{}")]
    input: String,
}

pub async fn run(client: &Client, args: Args) -> Result<(), Whatever> {
    let input: serde_json::Value =
        serde_json::from_str(&args.input).whatever_context("--input must be a JSON object")?;
    let Some(arguments) = input.as_object().cloned() else {
        snafu::whatever!("--input must be a JSON object");
    };
    let result =
        client.invoke_tool(&args.name, arguments).await.whatever_context("invoking tool")?;
    if result.is_error == Some(true) {
        println!("The tool returned an error:");
    } else {
        println!("Response from tool:");
    }
    for item in &result.content {
        match item["type"].as_str().unwrap_or("") {
            "text" => println!("{}", item["text"].as_str().unwrap_or("")),
            "resource_link" => println!(
                "[resource_link] {} {}",
                item["uri"].as_str().unwrap_or(""),
                item["name"].as_str().unwrap_or("")
            ),
            other => {
                println!("[{other}] {}", serde_json::to_string_pretty(item).unwrap_or_default());
            }
        }
    }
    if let Some(sc) = &result.structured_content {
        println!("\nStructured content:\n{}", serde_json::to_string_pretty(sc).unwrap_or_default());
    }
    Ok(())
}
