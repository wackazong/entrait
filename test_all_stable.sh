#! /bin/sh
set -e
set -x

cargo hack --feature-powerset --optional-deps "unimock" --exclude-features "default use-associated-future" --exclude-no-default-features test
cargo test --workspace --features "unimock use-async-trait"
cargo test --doc --features "unimock use-async-trait"
