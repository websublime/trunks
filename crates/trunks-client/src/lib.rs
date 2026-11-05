#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

//! HTTP client for the Trunks management REST API (`/api/v0`). Used by the CLI; public.

use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;
use snafu::{ResultExt, Snafu};
use trunks_types::api::{
    CreateMcpClientRequest, CreateUserRequest, EnableDisableResult, InitRequest, InitResponse,
    McpClient, McpClientWithToken, McpServer, Prompt, PromptRenderResult, RegisterServerInput,
    RegisterServerResult, Resource, ResourceReadResult, ServerMetadata, Tool, ToolGroup,
    ToolGroupWithEndpoints, ToolInvokeResult, UpdateMcpClientRequest, UpdateToolGroupResponse,
    UpdateUserRequest, User, UserWithToken,
};
use trunks_types::{ApiErrorResponse, ServerMode};

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("invalid registry URL '{url}': {source}"))]
    Url { url: String, source: url::ParseError },
    #[snafu(display("request failed: {source}"))]
    Http { source: reqwest::Error },
    /// The server answered with an error envelope.
    #[snafu(display("{}", body.error))]
    Api { status: u16, body: ApiErrorResponse },
    #[snafu(display("request failed with status: {status}, message: {message}"))]
    Status { status: u16, message: String },
}

impl Error {
    /// Machine-readable error code, when the server sent one.
    #[must_use]
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Api { body, .. } => body.code.as_deref(),
            _ => None,
        }
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone)]
pub struct Client {
    base: url::Url,
    token: Option<SecretString>,
    http: reqwest::Client,
}

impl Client {
    pub fn new(base_url: &str, token: Option<SecretString>, http: reqwest::Client) -> Result<Self> {
        let mut base = url::Url::parse(base_url).context(UrlSnafu { url: base_url })?;
        if !base.path().ends_with('/') {
            base.set_path(&format!("{}/", base.path()));
        }
        Ok(Self { base, token, http })
    }

    #[must_use]
    pub fn base_url(&self) -> &url::Url {
        &self.base
    }

    /// A copy of this client authenticating with `token`.
    #[must_use]
    pub fn with_token(&self, token: SecretString) -> Self {
        Self { base: self.base.clone(), token: Some(token), http: self.http.clone() }
    }

    // ---- public
    pub async fn metadata(&self) -> Result<ServerMetadata> {
        self.get("metadata", &[]).await
    }

    pub async fn init(&self, mode: ServerMode) -> Result<InitResponse> {
        let url = self.join("init")?;
        let res =
            self.http.post(url).json(&InitRequest { mode }).send().await.context(HttpSnafu)?;
        Self::decode(res).await
    }

    // ---- /api/v0, user level
    pub async fn list_servers(&self) -> Result<Vec<McpServer>> {
        self.get("api/v0/servers", &[]).await
    }

    pub async fn list_tools(&self, server: Option<&str>) -> Result<Vec<Tool>> {
        self.get("api/v0/tools", &opt("server", server)).await
    }

    pub async fn get_tool(&self, name: &str) -> Result<Tool> {
        self.get("api/v0/tool", &[("name", name)]).await
    }

    pub async fn list_prompts(&self, server: Option<&str>) -> Result<Vec<Prompt>> {
        self.get("api/v0/prompts", &opt("server", server)).await
    }

    pub async fn get_prompt(&self, name: &str) -> Result<Prompt> {
        self.get("api/v0/prompt", &[("name", name)]).await
    }

    pub async fn list_resources(&self, server: Option<&str>) -> Result<Vec<Resource>> {
        self.get("api/v0/resources", &opt("server", server)).await
    }

    /// `POST /api/v0/tools/invoke`
    pub async fn invoke_tool(
        &self,
        name: &str,
        arguments: serde_json::Map<String, serde_json::Value>,
    ) -> Result<ToolInvokeResult> {
        let mut body = arguments;
        body.insert("name".into(), serde_json::Value::String(name.to_owned()));
        let url = self.join("api/v0/tools/invoke")?;
        let res =
            self.authorized(self.http.post(url).json(&body)).send().await.context(HttpSnafu)?;
        Self::decode(res).await
    }

    /// `POST /api/v0/prompts/render`
    pub async fn render_prompt(
        &self,
        name: &str,
        arguments: serde_json::Map<String, serde_json::Value>,
    ) -> Result<PromptRenderResult> {
        self.post(
            "api/v0/prompts/render",
            &[],
            &serde_json::json!({"name": name, "arguments": arguments}),
        )
        .await
    }

    /// `POST /api/v0/resources/read`
    pub async fn read_resource(&self, uri: &str) -> Result<ResourceReadResult> {
        self.post("api/v0/resources/read", &[], &serde_json::json!({ "uri": uri })).await
    }

    pub async fn whoami(&self) -> Result<User> {
        self.get("api/v0/users/whoami", &[]).await
    }

    // ---- /api/v0, admin level
    /// `POST /api/v0/servers?force=`
    pub async fn register_server(
        &self,
        input: &RegisterServerInput,
        force: bool,
    ) -> Result<RegisterServerResult> {
        let q: &[(&str, &str)] = if force { &[("force", "true")] } else { &[] };
        self.post("api/v0/servers", q, input).await
    }

    /// `DELETE /api/v0/servers/{name}`
    pub async fn deregister_server(&self, name: &str) -> Result<()> {
        self.delete(&format!("api/v0/servers/{name}")).await
    }

    /// `POST /api/v0/servers/{name}/enable|disable`
    pub async fn set_server_enabled(
        &self,
        name: &str,
        enabled: bool,
    ) -> Result<EnableDisableResult> {
        let action = if enabled { "enable" } else { "disable" };
        self.post(&format!("api/v0/servers/{name}/{action}"), &[], &()).await
    }

    /// `POST /api/v0/{tools|prompts|resources}/{enable|disable}?entity=`
    pub async fn set_entities_enabled(
        &self,
        kind: &str,
        entity: &str,
        enabled: bool,
    ) -> Result<EnableDisableResult> {
        let action = if enabled { "enable" } else { "disable" };
        self.post(&format!("api/v0/{kind}/{action}"), &[("entity", entity)], &()).await
    }

    /// `GET /api/v0/server_configs` — registration inputs **with secrets** (admin).
    pub async fn server_configs(&self) -> Result<Vec<RegisterServerInput>> {
        self.get("api/v0/server_configs", &[]).await
    }

    // ---- /api/v0, tool groups (admin)
    pub async fn list_groups(&self) -> Result<Vec<ToolGroup>> {
        self.get("api/v0/tool-groups", &[]).await
    }

    pub async fn get_group(&self, name: &str) -> Result<ToolGroupWithEndpoints> {
        self.get(&format!("api/v0/tool-groups/{name}"), &[]).await
    }

    pub async fn create_group(&self, group: &ToolGroup) -> Result<ToolGroupWithEndpoints> {
        self.post("api/v0/tool-groups", &[], group).await
    }

    pub async fn update_group(
        &self,
        name: &str,
        group: &ToolGroup,
    ) -> Result<UpdateToolGroupResponse> {
        self.put(&format!("api/v0/tool-groups/{name}"), group).await
    }

    pub async fn delete_group(&self, name: &str) -> Result<()> {
        self.delete(&format!("api/v0/tool-groups/{name}")).await
    }

    /// `GET /api/v0/tool-groups/{name}/effective-tools`
    pub async fn group_tools(&self, name: &str) -> Result<Vec<Tool>> {
        self.get(&format!("api/v0/tool-groups/{name}/effective-tools"), &[]).await
    }

    // ---- /api/v0, enterprise + admin
    pub async fn list_users(&self) -> Result<Vec<User>> {
        self.get("api/v0/users", &[]).await
    }

    pub async fn create_user(&self, req: &CreateUserRequest) -> Result<UserWithToken> {
        self.post("api/v0/users", &[], req).await
    }

    pub async fn update_user(
        &self,
        username: &str,
        req: &UpdateUserRequest,
    ) -> Result<UserWithToken> {
        self.put(&format!("api/v0/users/{username}"), req).await
    }

    pub async fn delete_user(&self, username: &str) -> Result<()> {
        self.delete(&format!("api/v0/users/{username}")).await
    }

    pub async fn list_clients(&self) -> Result<Vec<McpClient>> {
        self.get("api/v0/clients", &[]).await
    }

    pub async fn create_client(&self, req: &CreateMcpClientRequest) -> Result<McpClientWithToken> {
        self.post("api/v0/clients", &[], req).await
    }

    pub async fn update_client(
        &self,
        name: &str,
        req: &UpdateMcpClientRequest,
    ) -> Result<McpClientWithToken> {
        self.put(&format!("api/v0/clients/{name}"), req).await
    }

    pub async fn delete_client(&self, name: &str) -> Result<()> {
        self.delete(&format!("api/v0/clients/{name}")).await
    }

    // ---- plumbing
    async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, &str)]) -> Result<T> {
        let url = self.join(path)?;
        let req = self.authorized(self.http.get(url).query(query));
        Self::decode(req.send().await.context(HttpSnafu)?).await
    }

    async fn post<B: serde::Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
        body: &B,
    ) -> Result<T> {
        let url = self.join(path)?;
        let req = self.authorized(self.http.post(url).query(query).json(body));
        Self::decode(req.send().await.context(HttpSnafu)?).await
    }

    async fn put<B: serde::Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let url = self.join(path)?;
        let req = self.authorized(self.http.put(url).json(body));
        Self::decode(req.send().await.context(HttpSnafu)?).await
    }

    async fn delete(&self, path: &str) -> Result<()> {
        let url = self.join(path)?;
        let res = self.authorized(self.http.delete(url)).send().await.context(HttpSnafu)?;
        Self::decode_empty(res).await
    }

    fn join(&self, path: &str) -> Result<url::Url> {
        self.base.join(path).context(UrlSnafu { url: path })
    }

    fn authorized(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.token {
            Some(t) => req.bearer_auth(t.expose_secret()),
            None => req,
        }
    }

    async fn decode<T: DeserializeOwned>(res: reqwest::Response) -> Result<T> {
        if res.status().is_success() {
            return res.json().await.context(HttpSnafu);
        }
        Err(Self::api_error(res).await)
    }

    async fn decode_empty(res: reqwest::Response) -> Result<()> {
        if res.status().is_success() {
            return Ok(());
        }
        Err(Self::api_error(res).await)
    }

    async fn api_error(res: reqwest::Response) -> Error {
        let code = res.status().as_u16();
        let text = res.text().await.unwrap_or_default();
        match serde_json::from_str::<ApiErrorResponse>(&text) {
            Ok(body) => Error::Api { status: code, body },
            Err(_) => Error::Status { status: code, message: text },
        }
    }
}

fn opt<'a>(key: &'a str, value: Option<&'a str>) -> Vec<(&'a str, &'a str)> {
    value.map(|v| vec![(key, v)]).unwrap_or_default()
}
