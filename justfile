default:
  just --list

lint:
  cargo clippy --all-targets -- -D warnings

lint-js:
  npm run lint

test:
  cargo test

fmt:
  npx prettier --write static/*.js

check: lint lint-js test

docker-build:
  docker build -t ghcr.io/yaleman/markdown-wrangler:latest .
