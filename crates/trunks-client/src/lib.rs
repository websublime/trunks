#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

//! HTTP client for the Trunks management REST API (`/api/v0`).
//!
//! Only `/metadata` exists in this phase; the rest of the surface is added
//! alongside each API handler.

use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use snafu::{ResultExt, Snafu};
use trunks_types::ApiErrorResponse;

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

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ServerMetadata {
    pub version: String,
}

#[derive(Debug, Clone)]
pub struct Client {
    base: url::Url,
    token: Option<SecretString>,
    http: reqwest::Client,
}

impl Client {
    pub fn new(base_url: &str, token: Option<SecretString>, http: reqwest::Client) -> Result<Self> {
        let base = url::Url::parse(base_url).context(UrlSnafu { url: base_url })?;
        Ok(Self { base, token, http })
    }

    #[must_use]
    pub fn base_url(&self) -> &url::Url {
        &self.base
    }

    /// `GET /metadata` — no authentication.
    pub async fn metadata(&self) -> Result<ServerMetadata> {
        let url = self.join("/metadata")?;
        let resp = self.http.get(url).send().await.context(HttpSnafu)?;
        Self::decode(resp).await
    }

    fn join(&self, path: &str) -> Result<url::Url> {
        self.base.join(path).context(UrlSnafu { url: path })
    }

    #[allow(dead_code)]
    fn authorized(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.token {
            Some(t) => req.bearer_auth(t.expose_secret()),
            None => req,
        }
    }

    async fn decode<T: serde::de::DeserializeOwned>(resp: reqwest::Response) -> Result<T> {
        let status = resp.status();
        if status.is_success() {
            return resp.json().await.context(HttpSnafu);
        }
        let code = status.as_u16();
        let text = resp.text().await.unwrap_or_default();
        match serde_json::from_str::<ApiErrorResponse>(&text) {
            Ok(body) => Err(Error::Api { status: code, body }),
            Err(_) => Err(Error::Status { status: code, message: text }),
        }
    }
}
