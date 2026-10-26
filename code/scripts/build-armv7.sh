#!/usr/bin/env bash
# Cross-compila worker e CLI para armv7 (32 bits, p.ex. Orange Pi) com zig como linker
# (cargo-zigbuild) numa imagem derivada de aic-rust:1. Saída: volume aic-target-armv7, em
# /t/<alvo sem o sufixo de glibc>/release.
#
#   bash code/scripts/build-armv7.sh                      # glibc 2.27 (Ubuntu 18.04 em diante)
#   TARGET=armv7-unknown-linux-musleabihf bash code/scripts/build-armv7.sh   # musl (imagem Alpine)
set -euo pipefail
TARGET=${TARGET:-armv7-unknown-linux-gnueabihf.2.27}
cd "$(dirname "$0")/../.."
if ! docker image inspect aic-rust-zig:1 >/dev/null 2>&1; then
  docker rm -f aic-zig-setup >/dev/null 2>&1 || true
  MSYS_NO_PATHCONV=1 docker run --name aic-zig-setup aic-rust:1 sh -c '
    apt-get update -qq && apt-get install -y -qq python3-pip >/dev/null &&
    pip3 install --break-system-packages -q ziglang &&
    cargo install cargo-zigbuild --locked &&
    rustup target add armv7-unknown-linux-gnueabihf armv7-unknown-linux-musleabihf'
  docker commit aic-zig-setup aic-rust-zig:1 >/dev/null
  docker rm aic-zig-setup >/dev/null
fi
# A imagem criada antes do alvo musl existir não o tem: adiciona sem refazer tudo.
MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/w" -v aic-target-armv7:/t \
  -e CARGO_TARGET_DIR=/t -w /w/code aic-rust-zig:1 sh -c "
    rustup target add armv7-unknown-linux-musleabihf >/dev/null 2>&1 || true
    cargo zigbuild --release --target $TARGET -j2 -p ai-cluster-worker -p ai-cluster-cli"
