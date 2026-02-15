git := require("git")
cargo := require("cargo")

default:
    just --list

# run the linter, tests, and format the code
check: clippy test fmt

# run clippy
clippy:
    cargo clippy --all-targets --quiet --workspace

# run rust tests
test:
    cargo test --quiet --workspace

# format the rust code
fmt:
    cargo fmt --all -- --check


# run shellcheck on scripts
lint-scripts:
    shellcheck *.sh
    shellcheck scripts/*.sh

set positional-arguments

@coverage_inner *args='':
    cargo tarpaulin --workspace \
        --exclude-files=src/main.rs \
        --exclude-files=src/logging/mod.rs \
        --exclude-files=src/logging/consoleexporter.rs \
        $@

# run coverage checks
coverage:
    just coverage_inner --out=Html
    @echo "Coverage report should be at file://$(pwd)/tarpaulin-report.html"

coveralls:
    just coverage_inner --out=Html --coveralls $COVERALLS_REPO_TOKEN
    @echo "Coverage report should be at https://coveralls.io/github/yaleman/markdown-wrangler?branch=$(git branch --show-current)"

# build the docker image
@docker_build *args='':
    docker buildx build \
        --load \
        --build-arg "GITHUB_SHA=$(git rev-parse HEAD)" \
        --platform linux/$(uname -m) \
        --tag ghcr.io/yaleman/markdown-wrangler:latest $@ \
        .

# build and run the docker image, mounting ./config as the config dir
docker_run: docker_build
    docker run --rm -it \
        -p 9000:9000 \
        --platform linux/$(uname -m) \
        --env "MDR_TLS_CERT=${MDR_TLS_CERT}" \
        --env "MDR_TLS_KEY=${MDR_TLS_KEY}" \
        --env "MDR_FRONTEND_URL=${MDR_FRONTEND_URL}" \
        --env "MDR_OIDC_CLIENT_ID=${MDR_OIDC_CLIENT_ID}" \
        --env "MDR_OIDC_DISCOVERY_URL=${MDR_OIDC_DISCOVERY_URL}" \
        --env "MDR_LISTENER_ADDRESS=${MDR_LISTENER_ADDRESS}" \
        --mount type=bind,src=$(pwd),target=/data/ \
        ghcr.io/yaleman/markdown-wrangler:latest

run:
    cargo run --

run_debug:
    RUST_LOG=debug cargo run

# run mdbook in "serve" mode
serve_docs:
    cd docs && mdbook serve

@semgrep *args='':
    semgrep ci --config auto \
    --exclude-rule "yaml.github-actions.security.third-party-action-not-pinned-to-commit-sha.third-party-action-not-pinned-to-commit-sha" $@

lint_js:
    pnpm eslint static/*.js


jaeger:
    docker run --rm --name jaeger \
        -d \
        -p 16686:16686 \
        -p 4317:4317 \
        -p 4318:4318 \
        -p 5778:5778 \
        -p 9411:9411 \
        --platform linux/amd64 \
        cr.jaegertracing.io/jaegertracing/jaeger:2.11.0
    echo "Jaeger UI should be at http://localhost:16686"
