# TODO

## P0 - Security and correctness

- [ ] Fix delete from image/file preview pages by adding CSRF token fields to their hidden delete forms.
  - `generate_image_preview_html()` and `generate_file_preview_html()` currently omit `csrf_token`, but `POST /delete` requires it.
- [ ] Add integration tests for preview-page delete forms.
  - Assert that `GET /preview` and `GET /file-preview` include `name="csrf_token"` in the hidden delete form.
  - Add end-to-end delete test that starts from preview context and succeeds with valid token.
- [ ] Decide CSRF signing approach and align implementation/docs.
  - Either implement true HMAC-SHA256, or document current keyed-SHA256 behavior explicitly.

## P1 - Documentation and architecture accuracy

- [ ] Update `AGENTS.md` and `README.md` to match current code structure.
  - Logging module is `src/logging/mod.rs` (not `src/log_wrangler.rs`).
  - HTML is generated with string builders in `src/web.rs`; Askama templates are not used.
  - Route list should include `/file-info` and `/file-content`.
- [ ] Reconcile command docs with actual task names.
  - `just clippy`, `just test`, `just fmt`, `just check`, and `just lint_js`.
  - JS lint command is `pnpm run lint` (script exists in `package.json`).

## P2 - Frontend cleanup and policy consistency

- [ ] Remove inline JavaScript from server-generated HTML.
  - Move image-dimension script from `generate_image_preview_html()` into a static asset.
- [ ] Add/adjust frontend tests or lint rules to prevent reintroducing inline JS/CSS policy violations.

## P3 - Dependency hygiene

- [ ] Remove unused dependencies or adopt them intentionally.
  - `askama` and `askama_web` are declared but not used in current source.
