// The web console is a static site: `astro build` writes files that the `trunks` binary
// embeds and serves (docs/08-web-app.md). There is no server-side rendering.
import { defineConfig } from 'astro/config';

// Paths that belong to the Trunks server; `astro dev` forwards them to a local `trunks start`.
const SERVER = process.env.TRUNKS_DEV_SERVER ?? 'http://127.0.0.1:8080';
const proxy = Object.fromEntries(
  ['/api', '/auth', '/mcp', '/v0', '/health', '/metadata', '/metrics', '/init'].map((path) => [
    path,
    // The Host header stays the dev server's, so the session cookie and the server's
    // same-origin check behave as they do in production.
    { target: SERVER, changeOrigin: false },
  ]),
);

export default defineConfig({
  output: 'static',
  trailingSlash: 'always',
  build: {
    format: 'directory',
    // The server sends a strict Content-Security-Policy (`script-src 'self'; style-src
    // 'self'`), so nothing may be inlined into the pages.
    inlineStylesheets: 'never',
  },
  vite: {
    build: { assetsInlineLimit: 0 },
    server: { proxy },
  },
});
