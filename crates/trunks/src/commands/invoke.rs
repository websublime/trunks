//! `trunks invoke <tool> --input '{...}'`

use crate::out::outln;
use base64::Engine as _;
use clap::Args as ClapArgs;
use snafu::{ResultExt, Whatever};
use trunks_client::Client;

/// File extension for a MIME type of binary tool output.
fn extension(mime: Option<&str>) -> &'static str {
    match mime.map(|m| m.split(';').next().unwrap_or(m).trim()) {
        Some("image/png") => "png",
        Some("image/jpeg" | "image/jpg") => "jpg",
        Some("image/gif") => "gif",
        Some("image/webp") => "webp",
        Some("image/svg+xml") => "svg",
        Some("audio/mpeg" | "audio/mp3") => "mp3",
        Some("audio/wav" | "audio/x-wav") => "wav",
        Some("audio/ogg") => "ogg",
        Some("application/pdf") => "pdf",
        Some("application/json") => "json",
        Some("text/plain") => "txt",
        _ => "bin",
    }
}

/// Decodes a base64 payload into `<prefix>_<unix-ms>.<ext>` in the current directory.
pub fn save_blob(
    prefix: &str,
    mime: Option<&str>,
    data: &str,
) -> Result<std::path::PathBuf, Whatever> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .whatever_context("content is not valid base64")?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let path = std::path::PathBuf::from(format!("{prefix}_{ts}.{}", extension(mime)));
    std::fs::write(&path, bytes).whatever_context(format!("writing {}", path.display()))?;
    Ok(path)
}

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Canonical tool name (<server>__<tool>)
    name: String,
    /// JSON object with the tool's arguments
    #[arg(long, default_value = "{}")]
    input: String,
    /// Only allow the call if the tool is an effective tool of this group
    #[arg(long)]
    group: Option<String>,
}

pub async fn run(client: &Client, args: Args) -> Result<(), Whatever> {
    let input: serde_json::Value =
        serde_json::from_str(&args.input).whatever_context("--input must be a JSON object")?;
    let Some(arguments) = input.as_object().cloned() else {
        snafu::whatever!("--input must be a JSON object");
    };
    if let Some(group) = &args.group {
        let tools = client.group_tools(group).await.whatever_context("resolving tool group")?;
        if !tools.iter().any(|t| t.name.to_string() == args.name) {
            snafu::whatever!("tool '{}' is not an effective tool of group '{group}'", args.name);
        }
    }
    let result =
        client.invoke_tool(&args.name, arguments).await.whatever_context("invoking tool")?;
    if result.is_error == Some(true) {
        outln!("The tool returned an error:");
    } else {
        outln!("Response from tool:");
    }
    for item in &result.content {
        match item["type"].as_str().unwrap_or("") {
            "text" => outln!("{}", item["text"].as_str().unwrap_or("")),
            "resource_link" => outln!(
                "[resource_link] {} {}",
                item["uri"].as_str().unwrap_or(""),
                item["name"].as_str().unwrap_or("")
            ),
            kind @ ("image" | "audio") => {
                let path = save_blob(
                    kind,
                    item["mimeType"].as_str(),
                    item["data"].as_str().unwrap_or(""),
                )?;
                outln!("[{kind}] saved to {}", path.display());
            }
            "resource" => {
                let res = &item["resource"];
                if let Some(t) = res["text"].as_str() {
                    outln!("[resource {}]\n{t}", res["uri"].as_str().unwrap_or(""));
                } else if let Some(b) = res["blob"].as_str() {
                    let path = save_blob("resource", res["mimeType"].as_str(), b)?;
                    outln!(
                        "[resource {}] saved to {}",
                        res["uri"].as_str().unwrap_or(""),
                        path.display()
                    );
                }
            }
            other => {
                outln!("[{other}] {}", serde_json::to_string_pretty(item).unwrap_or_default());
            }
        }
    }
    if let Some(sc) = &result.structured_content {
        outln!("\nStructured content:\n{}", serde_json::to_string_pretty(sc).unwrap_or_default());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn maps_mime_to_extension() {
        assert_eq!(super::extension(Some("image/png")), "png");
        assert_eq!(super::extension(Some("audio/mpeg; rate=44100")), "mp3");
        assert_eq!(super::extension(Some("application/octet-stream")), "bin");
        assert_eq!(super::extension(None), "bin");
    }
}
