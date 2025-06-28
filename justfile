default:
  just --list

lint:
  cargo clippy --all-targets

test:
  cargo test

check: lint test
