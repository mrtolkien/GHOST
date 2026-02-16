# T-KOMA Development Commands
# Use: just fmt (format all), just check (verify build), just ci (full CI)

# Default recipe: show available commands
default:
    @just --list

# Format all Rust code
fmt-rust:
    cargo fmt --all

# Format all markdown, SQL, JSON, and TOML files using dprint
fmt-other:
    dprint fmt

# Format everything (Rust + Markdown/SQL/JSON/TOML)
fmt: fmt-rust fmt-other

# Run cargo check
check:
    cargo check --all-features --all-targets

# Run cargo clippy
clippy:
    cargo clippy --all-features --all-targets

# Run tests (excluding live tests)
test:
    cargo test

# Run all checks (format, clippy, test)
ci: fmt check clippy test

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

# Build documentation with mdbook
doc:
    mdbook build docs/

# Serve documentation locally with live reload
doc-serve:
    mdbook serve docs/
