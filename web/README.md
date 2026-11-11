# web/

Home of the Trunks web application: a static [Astro](https://astro.build) site built with
[`line://ui`](https://line-ui.websublime.com) Web Components (Lit + Zag.js), with no UI
framework on top. The definition is in `docs/08-web-app.md` (decision D18); the delivery plan
is Phase 6c of `docs/03-plano.md`.

The production build is copied into `crates/trunks-dashboard/dist` by
`cargo xtask build-dashboard` (Node 22) and embedded into the `trunks` binary.
