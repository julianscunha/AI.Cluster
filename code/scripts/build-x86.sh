#!/usr/bin/env bash
# Compila o worker e a CLI para x86_64 com musl (imagem Alpine do AI Cluster OS x86_64), usando a
# imagem aic-rust-zig:1 (zig como linker; criada por build-armv7.sh). Saída: volume aic-target-x86,
# em /t/x86_64-unknown-linux-musl/release.
set -euo pipefail
cd "$(dirname "$0")/../.."
docker image inspect aic-rust-zig:1 >/dev/null 2>&1 || { echo "rode antes code/scripts/build-armv7.sh (cria aic-rust-zig:1)" >&2; exit 2; }
MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/w" -v aic-target-x86:/t \
  -e CARGO_TARGET_DIR=/t -w /w/code aic-rust-zig:1 sh -c "
    rustup target add x86_64-unknown-linux-musl >/dev/null 2>&1 || true
    cargo zigbuild --release --target x86_64-unknown-linux-musl -j2 -p ai-cluster-worker -p ai-cluster-cli"
