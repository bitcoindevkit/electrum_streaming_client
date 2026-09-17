alias b := build
alias c := check
alias f := fmt
alias t := test
alias p := pre-push
alias d := doc

_default:
  @just --list

# Build the crate with the committed lockfile
build:
  cargo build --locked

# Format all code (modifies files)
fmt:
  cargo fmt --all

# Check formatting, library builds, Clippy, and commit signature
check:
  cargo fmt --all --check
  cargo build --locked --lib --no-default-features
  cargo build --locked --lib --all-features
  cargo clippy --locked --all-targets --no-default-features -- -D warnings
  cargo clippy --locked --all-targets --all-features -- -D warnings
  @[ "$(git log --pretty='format:%G?' -1 HEAD)" = "N" ] && \
      echo "\n⚠️  Unsigned commit: BDK requires that commits be signed." || \
      true

# Run all-target tests and doctests under both feature configurations
test:
  cargo test --locked --all-targets --no-default-features
  cargo test --locked --doc --no-default-features
  cargo test --locked --all-targets --all-features
  cargo test --locked --doc --all-features

# Check documentation under both feature configurations
doc:
  RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --no-default-features
  RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --all-features

# Run the full pre-push suite without modifying tracked files
pre-push: check test doc
