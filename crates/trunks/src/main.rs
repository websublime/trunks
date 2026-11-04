#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

//! `trunks` — the single binary: `trunks start` runs the gateway; every other
//! subcommand is a client of its REST API.

mod commands;
mod config;

use clap::{Parser, Subcommand};
use secrecy::SecretString;
use snafu::{ResultExt, Whatever};

const BANNER: &str = r"
 _____                 _
|_   _| __ _   _ _ __ | | _____
  | || '__| | | | '_ \| |/ / __|
  | || |  | |_| | | | |   <\__ \
  |_||_|   \__,_|_| |_|_|\_\___/
";

const DEFAULT_REGISTRY: &str = "http://127.0.0.1:8080";

#[derive(Debug, Parser)]
#[command(
    name = "trunks",
    about = "MCP Gateway for AI Agents",
    version,
    disable_help_subcommand = true
)]
struct Cli {
    /// Base URL of the Trunks server (overrides the config file)
    #[arg(long, global = true, env = "TRUNKS_REGISTRY")]
    registry: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Start the Trunks gateway server
    Start(commands::start::Args),
    /// List servers, tools, prompts or resources
    List(commands::list::Args),
    /// Show how to call a tool (input schema)
    Usage(commands::usage::Args),
    /// Invoke a tool through the gateway
    Invoke(commands::invoke::Args),
    /// Initialize a server running in enterprise mode (creates the admin user)
    InitServer,
    /// Store a user access token for this registry
    Login(commands::auth::LoginArgs),
    /// Print the CLI and server versions
    Version,
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    // `.env` is optional; a missing file is not an error.
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();
    match run(cli).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {}", snafu::Report::from_error(e));
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<(), Whatever> {
    if let Command::Start(args) = cli.command {
        return commands::start::run(args).await;
    }

    let cfg = config::load();
    let registry = cli.registry.or(cfg.registry_url).unwrap_or_else(|| DEFAULT_REGISTRY.to_owned());
    let token = cfg.access_token.map(SecretString::from);
    let client = trunks_client::Client::new(&registry, token, reqwest::Client::new())
        .whatever_context("invalid registry URL")?;

    match cli.command {
        Command::Start(_) => unreachable!("handled above"),
        Command::List(args) => commands::list::run(&client, args).await,
        Command::Usage(args) => commands::usage::run(&client, args).await,
        Command::Invoke(args) => commands::invoke::run(&client, args).await,
        Command::InitServer => commands::auth::init_server(&client, &registry).await,
        Command::Login(args) => commands::auth::login(&client, &registry, args).await,
        Command::Version => {
            println!("{BANNER}");
            println!("CLI Version: {}", trunks_types::version::version());
            match tokio::time::timeout(std::time::Duration::from_secs(5), client.metadata()).await {
                Ok(Ok(m)) => println!("Server Version: {}", m.version),
                _ => println!("Couldn't retrieve Server version at this time"),
            }
            println!("Server URL: {registry}");
            Ok(())
        }
    }
}
