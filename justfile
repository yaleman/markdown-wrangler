default:
  just --list

lint:
  cargo clippy --all-targets

lint-js:
  npm run lint

test:
  cargo test

check: lint lint-js test
