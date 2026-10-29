#!/usr/bin/env bash
# Empacota o AI Cluster inteiro para um Linux x86_64: controller, gateway, worker, CLI, llama-server,
# ggml-rpc-server, bootstrap.sh e o `aic-setup`. Os binários saem da imagem única do Compose.
#
#   docker compose -f deploy/docker-compose.yml build      # gera a imagem ai-cluster:local
#   bash code/scripts/package-cluster.sh [imagem] [saida]  # padrão: ai-cluster:local, dist/
#
# No servidor (Debian/Ubuntu 12+ ou glibc 2.36+; precisa de openssl, libgomp1, iproute2, python3):
#   tar xzf ai-cluster-x86_64.tar.gz && cd ai-cluster-x86_64 && sudo ./aic-setup server
set -euo pipefail
IMAGE=${1:-ai-cluster:local}
OUT=${2:-dist}
HERE=$(cd "$(dirname "$0")/.." && pwd)
NAME=ai-cluster-x86_64
mkdir -p "$OUT"
STAGE="$OUT/.stage-$$"
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/$NAME"
CID=$(docker create "$IMAGE")
trap 'docker rm -f "$CID" >/dev/null 2>&1; rm -rf "$STAGE"' EXIT
for bin in ai-cluster-controller ai-cluster-gateway ai-cluster-worker ai-cluster llama-server ggml-rpc-server; do
  docker cp "$CID:/usr/local/bin/$bin" "$STAGE/$NAME/$bin"
done
cp "$HERE/os/bootstrap.sh" "$HERE/os/aic-setup" "$STAGE/$NAME/"
# O NTFS do Windows perde o bit de execução: o modo é forçado e as quebras de linha são as do Unix.
sed -i 's/\r$//' "$STAGE/$NAME/aic-setup" "$STAGE/$NAME/bootstrap.sh"
tar --mode=755 --owner=0 --group=0 -C "$STAGE" -czf "$OUT/$NAME.tar.gz" "$NAME"
echo "pacote: $OUT/$NAME.tar.gz ($(du -h "$OUT/$NAME.tar.gz" | cut -f1))"
tar -tzf "$OUT/$NAME.tar.gz"
