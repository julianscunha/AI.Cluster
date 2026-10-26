#!/usr/bin/env bash
# Cross-compila worker e CLI para armv7 (32 bits, p.ex. Orange Pi/Raspberry de 32 bits) com glibc
# 2.27, que roda do Ubuntu 18.04 em diante. Usa zig como linker (cargo-zigbuild) numa imagem
# derivada de aic-rust:1. Saída: volume aic-target-armv7, em /t/armv7-unknown-linux-gnueabihf/release.
#
#   bash code/scripts/build-armv7.sh
set -euo pipefail
cd "$(dirname "$0")/../.."
if ! docker image inspect aic-rust-zig:1 >/dev/null 2>&1; then
  docker rm -f aic-zig-setup >/dev/null 2>&1 || true
  MSYS_NO_PATHCONV=1 docker run --name aic-zig-setup aic-rust:1 sh -c '
    apt-get update -qq && apt-get install -y -qq python3-pip >/dev/null &&
    pip3 install --break-system-packages -q ziglang &&
    cargo install cargo-zigbuild --locked &&
    rustup target add armv7-unknown-linux-gnueabihf'
  docker commit aic-zig-setup aic-rust-zig:1 >/dev/null
  docker rm aic-zig-setup >/dev/null
fi
MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/w" -v aic-target-armv7:/t \
  -e CARGO_TARGET_DIR=/t -w /w/code aic-rust-zig:1 \
  cargo zigbuild --release --target armv7-unknown-linux-gnueabihf.2.27 -j2 \
  -p ai-cluster-worker -p ai-cluster-cli
