# web/

Home of the Trunks web application, built with [`line://ui`](https://line-ui.websublime.com)
(Lit + Zag.js Web Components). See `docs/06-roadmap.md` and `docs/04-plano.md` (Fase 6).

The production bundle is copied into `crates/trunks-dashboard/dist` by
`cargo xtask build-dashboard` and embedded into the `trunks` binary.
