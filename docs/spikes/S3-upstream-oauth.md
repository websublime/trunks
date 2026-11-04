# S3 — OAuth upstream com `rmcp::transport::auth`

> 2026-11-04 · `rmcp` 3.8.0 (feature `auth`) · `wiremock` como authorization server e upstream · código em `spikes/s3-upstream-oauth/`

## Pergunta
A spec (§4.8) descreve um fluxo em duas fases: `register` falha com 401 → discovery → DCR ou
`client_id` pré-registado → URL de autorização com PKCE e `state` persistidos → (browser) →
`complete(code, state)` → troca → tokens persistidos → ligações seguintes usam o bearer e fazem
refresh. Tudo tem de sobreviver a um restart entre as fases e viver nas nossas tabelas.
Plano B: implementar sobre o crate `oauth2`.

## Resultado: o `rmcp` cobre o fluxo inteiro; sem plano B
| Peça da spec | API `rmcp` | Verificado |
|---|---|---|
| 401 do upstream com `WWW-Authenticate` | `StreamableHttpError::AuthRequired(AuthRequiredError { www_authenticate_header })` | ok |
| discovery (RFC 9728 → RFC 8414) | `AuthorizationManager::new_allowing_private_addresses(url)` + `resolve_metadata_from_challenge(Some(hdr))` / `resolve_metadata()` | ok |
| DCR (RFC 7591) só quando não há `client_id` | `register_client(name, redirect, scopes)`; erro "Dynamic client registration not supported" sem `registration_endpoint` | ok |
| `client_id`/`client_secret` fornecidos | `configure_client(OAuthClientConfig::new(id, redirect).with_client_secret(s))`; `/register` nunca é chamado | ok |
| PKCE S256 + `state` + `resource` (RFC 8707) | `get_authorization_url(scopes)` gera tudo; `state` é a chave do `StateStore` | ok |
| persistência da sessão pendente | trait `StateStore { save(csrf, StoredAuthorizationState{pkce_verifier, csrf_token, expected_issuer, created_at, requested_scopes}), load, delete }` → `upstream_oauth_pending_sessions` | ok |
| restart entre fases | novo `AuthorizationManager` com as mesmas stores; `exchange_code_for_token(code, state)` lê o verifier da store | ok |
| `state` errado | rejeitado **antes** de qualquer pedido ao token endpoint | ok |
| persistência de tokens | trait `CredentialStore { load, save(StoredCredentials{client_id, token_response, granted_scopes, token_received_at, issuer}), clear }` → `upstream_oauth_tokens` | ok |
| uso nas ligações | `AuthClient::new(reqwest::Client, manager)` → `StreamableHttpClientTransport::with_client(auth_client, cfg)`; `initialize` + `tools/list` com o bearer | ok |
| refresh transparente | `get_access_token()`/`prepare_request` refrescam quando faltam < 30 s (`REFRESH_BUFFER_SECS`); credenciais actualizadas na store | ok |
| refresh rejeitado (`invalid_grant`) | erro `AuthorizationRequired` + callback `on_refresh_token_rejected` na store → registry marca o servidor como "re-autorizar" | ok |

## Consequências para a implementação (`trunks-core::oauth`, `trunks-db::repos::oauth`)
1. As duas tabelas herdadas são o *shape* certo. `StoredAuthorizationState` → colunas `state`,
   `code_verifier`, `expires_at` (= `created_at` + TTL), `scopes`; `StoredCredentials` → `client_id`,
   `client_secret`, `access_token`, `refresh_token`, `token_type`, `scope`, `expires_at`
   (= `token_received_at` + `expires_in`). Falta uma coluna nova: `issuer` (RFC 9207) — migração aditiva
   `0004_oauth_issuer`.
2. `StateStore`/`CredentialStore` são `#[async_trait]`; as nossas impls recebem `(Db, ServerName)`.
3. O `session_id` que devolvemos ao CLI é nosso (64 hex); o `state` é gerado pelo `rmcp`. Guardam-se
   ambos na sessão pendente; `complete` carrega por `session_id` e passa `state` ao `rmcp`, que o valida.
4. Um `AuthorizationManager` por servidor OAuth, criado no `UpstreamConn::connect` com
   `initialize_from_store()`; `AuthClient` embrulha o `reqwest::Client` que já leva os headers configurados.
5. Hosts privados: usar `new_allowing_private_addresses` (necessário para IdPs internos e testes); o
   default rejeita RFC 1918.
6. Dependências: `async-trait` (impls das stores) e `oauth2` 5 (só para o trait `TokenResponse` nos
   acessos ao token) — ambas já transitivas do `rmcp`.
