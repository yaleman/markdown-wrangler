default:
  just --list

lint:
  cargo clippy --all-targets

lint-js:
  deno lint static/*.js

test:
  cargo test

check: lint lint-js test
