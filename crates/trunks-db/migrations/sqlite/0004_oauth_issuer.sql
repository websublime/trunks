-- 0004: RFC 9207 issuer of the stored OAuth credentials (rmcp validates it on reuse).
ALTER TABLE upstream_oauth_tokens ADD COLUMN issuer TEXT;
