//! `web/app/src/api/types.gen.ts`: the TypeScript declarations of the API types, generated
//! from the Rust types so the web app cannot drift from the contracts.
//!
//! `cargo xtask gen-types` writes the file; the test below fails when it is out of date.

use std::path::{Path, PathBuf};

use trunks_core::dashboard as dash;
use trunks_types::api;
use ts_rs::{Config, TS};

const HEADER: &str = "\
// Generated from the Rust API types (trunks-types, trunks-core::dashboard).
// Do not edit by hand: cargo xtask gen-types

/** Any JSON value: schemas, annotations and other content the gateway passes through. */
export type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue };
";

fn path(root: &Path) -> PathBuf {
    root.join("web/app/src/api/types.gen.ts")
}

macro_rules! declarations {
    ($cfg:expr; $($ty:ty),* $(,)?) => {
        vec![$(<$ty as TS>::decl($cfg)),*]
    };
}

fn generated() -> String {
    // JSON numbers are numbers: none of these values needs more than 53 bits.
    let cfg = &Config::new().with_large_int("number");
    let declarations = declarations![cfg;
        // names and enums
        trunks_types::ServerName, trunks_types::CanonicalName, trunks_types::ResourceUri,
        trunks_types::GroupName, trunks_types::Transport, trunks_types::SessionMode,
        trunks_types::Role, trunks_types::ServerMode,
        // service and sign-in
        trunks_types::error::ApiErrorResponse, api::Health, api::ServerMetadata, api::AuthConfig,
        api::InitRequest, api::InitResponse,
        // catalog
        api::McpServer, api::Tool, api::Prompt, api::Resource,
        // registration
        api::RegisterServerInput, api::RegisterServerResult, api::AuthorizationRequired,
        api::CompleteOAuthRequest, api::EnableDisableResult,
        // tool groups
        api::ToolGroup, api::ToolGroupWithEndpoints, api::UpdateToolGroupResponse,
        // identities and access
        api::User, api::CreateUserRequest, api::UpdateUserRequest, api::UserWithToken,
        api::Owner, api::McpClient, api::CreateMcpClientRequest, api::UpdateMcpClientRequest,
        api::McpClientWithToken, api::Team, api::TeamMember, api::TeamDetail,
        api::AddTeamMemberRequest, api::GrantSpec, api::AccessView, api::TeamMembership,
        api::WhoAmI,
        // dashboard views
        dash::Counts, dash::SessionStats, dash::Overview, dash::ConfigSummary, dash::OAuthStatus,
        dash::OAuthSummary, dash::EntityCounts, dash::ServerSummary, dash::ServerDetail,
        dash::GroupSummary, dash::UnreachableServer, dash::PendingOAuth, dash::DatabaseInfo,
        dash::BindInfo, dash::Diagnostics, dash::OAuthSessionState, dash::OAuthSessionStatus,
    ];
    let mut out = String::from(HEADER);
    for declaration in declarations {
        out.push('\n');
        out.push_str("export ");
        out.push_str(declaration.trim_end());
        out.push('\n');
    }
    out
}

/// Writes the declarations.
pub fn write(root: &Path) -> Result<(), String> {
    let path = path(root);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, generated()).map_err(|e| e.to_string())?;
    println!("wrote {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_typescript_declarations_match_the_rust_types() {
        let root = crate::project_root();
        let on_disk =
            std::fs::read_to_string(path(&root)).unwrap_or_default().replace("\r\n", "\n");
        assert!(
            on_disk == generated(),
            "web/app/src/api/types.gen.ts is out of date; regenerate it with\n\
             cargo xtask gen-types"
        );
    }
}
