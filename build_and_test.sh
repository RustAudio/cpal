#!/usr/bin/env bash

set -eux

cargo fmt --all -- --check \
  && cargo clippy --all --all-features -- -D warnings \
  && cargo test --all --all-features