# TODO

## P0 - Security and correctness

- [ ] Add end-to-end delete test that starts from preview context and succeeds with a valid token.
  - Presence of `name="csrf_token"` in preview pages is already covered; remaining gap is full preview-to-delete flow.

## P1 - Documentation and architecture accuracy

- [ ] Update `AGENTS.md` and `README.md` to match current code structure.
  - Logging module is `src/logging/mod.rs` (not `src/log_wrangler.rs`).
  - HTML is rendered with Askama templates in `templates/` (`#[derive(Template, WebTemplate)]` in `src/web/mod.rs`).
  - Route list should include `/file-info` and `/file-content`.
- [ ] Reconcile command docs with actual task names.
  - `just clippy`, `just test`, `just fmt`, `just check`, and `just lint_js`.
  - JS lint command is `pnpm run lint` (script exists in `package.json`).

## P2 - Frontend cleanup and policy consistency

- [ ] Add/adjust frontend tests or lint rules to prevent reintroducing inline JS/CSS policy violations.
