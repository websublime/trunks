# Test fixtures

`test-idp-key.pem` is a throwaway RSA-2048 private key generated only for the fake identity
provider in `trunks_testing::oidc`. It signs ID tokens in tests and protects nothing.
`test-idp-key.n` is its public modulus (base64url), served in the fake JWKS.
