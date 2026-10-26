#!/usr/bin/env bash
# Monta a imagem base do AI Cluster OS (F15 J1) para armv7: o rootfs Alpine com o worker, como
# imagem ext4 (rootfs.ext4) pronta para ir a um slot. Sem montar nada no PC: o ext4 sai de
# `mke2fs -d` dentro de um contêiner.
#
#   bash code/os/image/build-rootfs.sh [tamanho-do-slot, padrão 512M]
#
# Requer o worker musl: TARGET=armv7-unknown-linux-musleabihf bash code/scripts/build-armv7.sh
set -euo pipefail
cd "$(dirname "$0")"
SLOT=${1:-512M}
mkdir -p build
MSYS_NO_PATHCONV=1 docker run --rm -v aic-target-armv7:/t -v "$(pwd -W 2>/dev/null || pwd)/build:/out" aic-rust-zig:1 \
  cp /t/armv7-unknown-linux-musleabihf/release/ai-cluster-worker /out/ai-cluster-worker
cp build/ai-cluster-worker ai-cluster-worker
docker build --platform linux/arm/v7 --build-arg "AIC_RELEASE=${AIC_RELEASE:-dev}" -t aic-os-rootfs:dev .
rm -f ai-cluster-worker
CID=$(docker create --platform linux/arm/v7 aic-os-rootfs:dev)
docker export "$CID" > build/rootfs.tar
docker rm "$CID" >/dev/null
# O tar é extraído como root dentro do contêiner, para preservar dono e modo, e vira ext4 sem montar.
MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd)/build:/b" debian:12-slim sh -c "
  apt-get update -qq >/dev/null && apt-get install -y -qq e2fsprogs >/dev/null &&
  rm -rf /r && mkdir /r && tar -xf /b/rootfs.tar -C /r && rm -f /r/.dockerenv &&
  rm -f /b/rootfs.ext4 && mke2fs -q -t ext4 -L aicroot -d /r -E root_owner=0:0 /b/rootfs.ext4 $SLOT &&
  e2fsck -fn /b/rootfs.ext4 | tail -1 && du -sh /r"
ls -l build/rootfs.ext4
