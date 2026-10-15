#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
CARGO_PROFILE_RELEASE_LTO=false cargo test -p effectcraft-ui-egui --release --lib panels::project::tests::size_sort_benchmark -- --ignored --nocapture
