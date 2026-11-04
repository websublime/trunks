//! `trunks register` — registers an upstream server from flags or a JSON file.

use std::collections::BTreeMap;

use clap::{Args as ClapArgs, ValueEnum};
use snafu::{ResultExt, Whatever};
use trunks_client::Client;
use trunks_types::api::RegisterServerInput;
use trunks_types::{SessionMode, Transport};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum TransportArg {
    Stdio,
    StreamableHttp,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SessionModeArg {
    Stateless,
    Stateful,
}

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// JSON file with the server definition (`${VAR}` references are expanded)
    #[arg(short = 'c', long = "conf", value_name = "FILE")]
    conf: Option<std::path::PathBuf>,
    /// Server name (letters, digits, `_` and `-`)
    #[arg(long)]
    name: Option<String>,
    #[arg(long, value_enum)]
    transport: Option<TransportArg>,
    #[arg(long)]
    description: Option<String>,
    /// streamable_http: upstream URL
    #[arg(long)]
    url: Option<String>,
    /// streamable_http: sent as `Authorization: Bearer …`
    #[arg(long)]
    bearer_token: Option<String>,
    /// streamable_http: extra header, `Name: value` or `Name=value` (repeatable)
    #[arg(long = "header", value_name = "NAME=VALUE")]
    headers: Vec<String>,
    /// stdio: program to run
    #[arg(long)]
    command: Option<String>,
    /// stdio: program argument (repeatable, in order)
    #[arg(long = "arg", value_name = "ARG")]
    argv: Vec<String>,
    /// stdio: environment variable `NAME=value` (repeatable)
    #[arg(long = "env", value_name = "NAME=VALUE")]
    env: Vec<String>,
    #[arg(long, value_enum)]
    session_mode: Option<SessionModeArg>,
    /// Replace an existing server of the same name
    #[arg(long)]
    force: bool,
}

/// Splits on the first `=` or `:`, whichever comes first (so `X-Key: a=b` keeps its value).
fn split_pair(s: &str) -> Result<(String, String), Whatever> {
    let Some(i) = s.find(['=', ':']) else {
        snafu::whatever!("expected NAME=VALUE, got '{s}'");
    };
    Ok((s[..i].trim().to_owned(), s[i + 1..].trim().to_owned()))
}

fn pairs(items: &[String]) -> Result<BTreeMap<String, String>, Whatever> {
    items.iter().map(|s| split_pair(s)).collect()
}

fn build_input(args: &Args) -> Result<RegisterServerInput, Whatever> {
    let mut input = match &args.conf {
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .whatever_context(format!("reading {}", path.display()))?;
            let mut value: serde_json::Value = serde_json::from_str(&text)
                .whatever_context(format!("{} is not valid JSON", path.display()))?;
            crate::envsubst::expand(&mut value)?;
            serde_json::from_value(value)
                .whatever_context(format!("{} is not a valid server definition", path.display()))?
        }
        None => RegisterServerInput {
            name: String::new(),
            transport: Transport::Stdio,
            description: String::new(),
            url: None,
            bearer_token: None,
            headers: BTreeMap::new(),
            command: None,
            args: vec![],
            env: BTreeMap::new(),
            session_mode: SessionMode::default(),
            oauth_redirect_uri: None,
            oauth_client_id: None,
            oauth_client_secret: None,
            oauth_scopes: vec![],
        },
    };
    // Flags override the file.
    if let Some(n) = &args.name {
        input.name.clone_from(n);
    }
    if let Some(t) = args.transport {
        input.transport = match t {
            TransportArg::Stdio => Transport::Stdio,
            TransportArg::StreamableHttp => Transport::StreamableHttp,
        };
    } else if args.conf.is_none() {
        input.transport =
            if args.url.is_some() { Transport::StreamableHttp } else { Transport::Stdio };
    }
    if let Some(d) = &args.description {
        input.description.clone_from(d);
    }
    if args.url.is_some() {
        input.url.clone_from(&args.url);
    }
    if args.bearer_token.is_some() {
        input.bearer_token.clone_from(&args.bearer_token);
    }
    if !args.headers.is_empty() {
        input.headers.extend(pairs(&args.headers)?);
    }
    if args.command.is_some() {
        input.command.clone_from(&args.command);
    }
    if !args.argv.is_empty() {
        input.args.clone_from(&args.argv);
    }
    if !args.env.is_empty() {
        input.env.extend(pairs(&args.env)?);
    }
    if let Some(m) = args.session_mode {
        input.session_mode = match m {
            SessionModeArg::Stateless => SessionMode::Stateless,
            SessionModeArg::Stateful => SessionMode::Stateful,
        };
    }
    if input.name.trim().is_empty() {
        snafu::whatever!("a server name is required (--name or \"name\" in the file)");
    }
    Ok(input)
}

pub async fn run(client: &Client, args: Args) -> Result<(), Whatever> {
    let input = build_input(&args)?;
    let name = input.name.clone();
    let result =
        client.register_server(&input, args.force).await.whatever_context("registering server")?;
    let Some(server) = result.server else {
        snafu::whatever!(
            "the server requires an upstream OAuth flow, which this version does not support"
        );
    };
    println!("Server {} registered successfully!", server.name);

    let tools = client.list_tools(Some(&name)).await.whatever_context("listing tools")?;
    let prompts = client.list_prompts(Some(&name)).await.whatever_context("listing prompts")?;
    let resources =
        client.list_resources(Some(&name)).await.whatever_context("listing resources")?;
    if tools.is_empty() && prompts.is_empty() && resources.is_empty() {
        println!("This server does not provide any tools, prompts or resources.");
        return Ok(());
    }
    if !tools.is_empty() {
        println!("\nTools ({}):", tools.len());
        for t in &tools {
            println!("  {}  {}", t.name, t.description);
        }
    }
    if !prompts.is_empty() {
        println!("\nPrompts ({}):", prompts.len());
        for p in &prompts {
            println!("  {}  {}", p.name, p.description);
        }
    }
    if !resources.is_empty() {
        println!("\nResources ({}):", resources.len());
        for r in &resources {
            println!("  {}  {}", r.uri, r.name);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_pairs() {
        assert_eq!(split_pair("A=1").unwrap(), ("A".into(), "1".into()));
        assert_eq!(split_pair("X-Key: v=1").unwrap(), ("X-Key".into(), "v=1".into()));
        assert_eq!(split_pair("URL=http://h:1").unwrap(), ("URL".into(), "http://h:1".into()));
        assert!(split_pair("novalue").is_err());
    }
}
