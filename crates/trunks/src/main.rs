#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

//! `trunks` — the single binary: `trunks start` runs the gateway; every other
//! subcommand is a client of its REST API.

mod commands;

use clap::{Parser, Subcommand};
use snafu::{ResultExt, Whatever};

const BANNER: &str = r"
 _____                 _
|_   _| __ _   _ _ __ | | _____
  | || '__| | | | '_ \| |/ / __|
  | || |  | |_| | | | |   <\__ \
  |_||_|   \__,_|_| |_|_|\_\___/
";

#[derive(Debug, Parser)]
#[command(
    name = "trunks",
    about = "MCP Gateway for AI Agents",
    version,
    disable_help_subcommand = true
)]
struct Cli {
    /// Base URL of the Trunks registry server
    #[arg(long, global = true, default_value = "http://127.0.0.1:8080", env = "TRUNKS_REGISTRY")]
    registry: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Start the Trunks gateway server
    Start(commands::start::Args),
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
    match cli.command {
        Command::Start(args) => commands::start::run(args).await,
        Command::Version => {
            println!("{BANNER}");
            println!("CLI Version: {}", trunks_types::version::version());
            let client = trunks_client::Client::new(&cli.registry, None, reqwest_client())
                .whatever_context("invalid registry URL")?;
            match tokio::time::timeout(std::time::Duration::from_secs(5), client.metadata()).await {
                Ok(Ok(m)) => println!("Server Version: {}", m.version),
                _ => println!("Couldn't retrieve Server version at this time"),
            }
            println!("Server URL: {}", cli.registry);
            Ok(())
        }
    }
}

fn reqwest_client() -> reqwest::Client {
    reqwest::Client::new()
}
