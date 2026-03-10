# T-KOMA Development Commands
# Use: just fmt (format all), just check (verify build), just ci (full CI)

# Default recipe: show available commands
default:
    @just --list

# Format all Rust code
fmt-rust:
    cargo fmt --all

fmt-lua:
    stylua .

# Format markdown, MDX, JSON, and TOML files using oxfmt
fmt-ox:
    npx oxfmt .

# Format everything (Rust + Markdown/MDX/JSON/TOML)
fmt: fmt-rust fmt-ox fmt-lua

# Run cargo check
check:
    rtk cargo check --all-features --all-targets

# Run cargo clippy
clippy:
    rtk cargo clippy --all-features --all-targets

# Run tests (excluding live tests)
test:
    rtk cargo test

# Run step-based e2e fixture refresh (manual, sequential)
e2e-refresh models="primary":
    uv run scripts/e2e refresh --models {{models}}

# Compile step-based tests without executing them
stepwise-check:
    rtk cargo test --features live-tests-llms --test stepwise --no-run

# Generate schema.sql from migrations
generate-schema:
    ./scripts/generate-schema.sh

# Check that schema.sql is up-to-date with migrations
check-schema:
    @./scripts/generate-schema.sh > /dev/null
    @git diff --exit-code assets/skills/knowledge-navigator/schema.sql || (echo "schema.sql is stale — run 'just generate-schema'" && exit 1)

# Run all checks (format, clippy, test)
ci: fmt check clippy test generate-schema

# Clean build artifacts
clean:
    cargo clean
    rm -rf target/

# Build in release mode
build-release:
    cargo build --release --all-features

# Run the gateway
run-gateway:
    cargo run --bin t-koma-gateway

# Run the CLI
run-cli:
    cargo run --bin t-koma-cli

# Build documentation with Astro Starlight
doc:
    cd docs && npm run build

# Serve documentation locally with live reload
doc-serve:
    cd docs && npm run dev
