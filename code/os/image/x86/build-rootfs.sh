#!/usr/bin/env bash
# Monta o rootfs x86_64 do AI Cluster OS (Alpine + linux-lts + initramfs + worker) como imagem ext4
# (build/rootfs-x86.ext4) pronta para um slot. Sem montar nada no PC.
#
#   bash code/os/image/x86/build-rootfs.sh [tamanho-do-slot, padrão 512M]
#
# Requer o worker musl x86_64 (bash code/scripts/build-x86.sh) e as chaves de release (release-keys.sh).
set -euo pipefail
cd "$(dirname "$0")/.."
SLOT=${1:-512M}
mkdir -p build
MSYS_NO_PATHCONV=1 docker run --rm -v aic-target-x86:/t -v "$(pwd -W 2>/dev/null || pwd)/build:/out" aic-rust-zig:1 \
  cp /t/x86_64-unknown-linux-musl/release/ai-cluster-worker /out/ai-cluster-worker-x86
# rpc-server do llama.cpp (ADR-022) estático em musl, com o patch de code/third_party/llama.cpp (build.sh
# o compila no volume aic-llama); sem símbolos.
MSYS_NO_PATHCONV=1 docker run --rm -v aic-llama:/w -v "$(pwd -W 2>/dev/null || pwd)/build:/out" aic-rust-zig:1 \
  sh -c 'apt-get update -qq >/dev/null && apt-get install -y -qq binutils >/dev/null && strip -o /out/ggml-rpc-server-x86 /w/llama.cpp/build-x86m/bin/ggml-rpc-server'
cp build/ggml-rpc-server-x86 ggml-rpc-server
# AIC_WORKER_FILE troca o binário do worker (teste de rollback com um worker que nunca se registra)
cp "${AIC_WORKER_FILE:-build/ai-cluster-worker-x86}" ai-cluster-worker
MSYS_NO_PATHCONV=1 docker run --rm -v aic-target-x86:/t -v "$(pwd -W 2>/dev/null || pwd)/build:/out" aic-rust-zig:1 \
  cp /t/x86_64-unknown-linux-musl/release/aic /out/aic-x86
cp build/aic-x86 aic
rm -rf aic-release.d; mkdir aic-release.d
for PUB in ${AIC_PUBS:-${AIC_KEYS:-$HOME/.aic}/release.pub}; do
  [ -f "$PUB" ] || { echo "falta $PUB (rode release-keys.sh)" >&2; exit 2; }
  cp "$PUB" "aic-release.d/$(basename "$PUB")"
done
docker build --platform linux/amd64 -f x86/Dockerfile --build-arg "AIC_RELEASE=${AIC_RELEASE:-dev}" -t aic-os-rootfs-x86:dev .
rm -rf ai-cluster-worker ggml-rpc-server aic aic-release.d
CID=$(docker create --platform linux/amd64 aic-os-rootfs-x86:dev)
docker export "$CID" > build/rootfs-x86.tar
docker rm "$CID" >/dev/null
MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd)/build:/b" debian:12-slim sh -c "
  apt-get update -qq >/dev/null && apt-get install -y -qq e2fsprogs >/dev/null &&
  rm -rf /r && mkdir /r && tar -xf /b/rootfs-x86.tar -C /r && rm -f /r/.dockerenv &&
  rm -f /b/rootfs-x86.ext4 && mke2fs -q -t ext4 -O ^has_journal,^orphan_file -L aicroot -d /r -E root_owner=0:0 /b/rootfs-x86.ext4 $SLOT &&
  e2fsck -fn /b/rootfs-x86.ext4 | tail -1 && du -sh /r && ls -l /r/boot"
ls -l build/rootfs-x86.ext4
