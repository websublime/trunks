//! `trunks init-server` and `trunks login`.

use clap::Args as ClapArgs;
use secrecy::SecretString;
use snafu::{ResultExt, Whatever};
use trunks_client::Client;
use trunks_types::ServerMode;

use crate::config::{self, CliConfig};
use crate::out::outln;

#[derive(Debug, ClapArgs)]
pub struct LoginArgs {
    /// Access token of a user (shown once by `init-server` or `create user`)
    #[arg(allow_hyphen_values = true)]
    token: String,
}

/// Initializes the server in enterprise mode and stores the admin token locally.
pub async fn init_server(client: &Client, registry: &str) -> Result<(), Whatever> {
    let res = client.init(ServerMode::Enterprise).await.whatever_context("initializing server")?;
    let Some(token) = res.admin_access_token.filter(|t| !t.is_empty()) else {
        snafu::whatever!("server did not return an admin access token");
    };
    let path = config::save(&CliConfig {
        registry_url: Some(registry.to_owned()),
        access_token: Some(token.clone()),
    })?;
    outln!("{}", res.status);
    outln!("Admin access token: {token}");
    outln!("Saved to {} — this token is shown only once.", path.display());
    Ok(())
}

pub async fn login(client: &Client, registry: &str, args: LoginArgs) -> Result<(), Whatever> {
    let user = client
        .with_token(SecretString::from(args.token.clone()))
        .whoami()
        .await
        .whatever_context("login failed")?;
    let path = config::save(&CliConfig {
        registry_url: Some(registry.to_owned()),
        access_token: Some(args.token),
    })?;
    outln!("You are now logged in as {} ({})", user.username, user.role);
    outln!("Credentials saved to {}", path.display());
    Ok(())
}
