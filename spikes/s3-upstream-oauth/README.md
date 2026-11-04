# Spike S3 — upstream OAuth with `rmcp::transport::auth`

Question: does the `rmcp` auth module cover the registry's two-phase upstream OAuth flow
(bootstrap → browser → complete) with state and credentials persisted in **our** tables, or do
we need the `oauth2` crate directly?

Answer: **it covers it.** `AuthorizationManager` + pluggable `StateStore`/`CredentialStore`
(mapped onto `upstream_oauth_pending_sessions` / `upstream_oauth_tokens`) + `AuthClient` as the
transport wrapper. Result: `docs/spikes/S3-upstream-oauth.md`.

```bash
cd spikes/s3-upstream-oauth && cargo run     # 22 checks against a wiremock AS + upstream
```
