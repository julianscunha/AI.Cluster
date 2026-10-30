#!/usr/bin/env bash
# Gera os arquivos de uma release, um por componente e arquitetura, com nomes SEM versão para o
# instalador achar a última release em releases/latest/download/<arquivo>.
#
#   docker compose -f deploy/docker-compose.yml build                 # imagem ai-cluster:local (x86_64)
#   TARGET=armv7-unknown-linux-musleabihf bash code/scripts/build-armv7.sh   # binários armv7
#   bash code/scripts/package-release.sh [saida]                      # padrão: dist/release
#
# Saída: install.sh, VERSION, tools.tar.gz, controller-linux-{x86_64,armv7}.tar.gz,
# gateway-linux-x86_64.tar.gz, worker-linux-{x86_64,armv7}.tar.gz e SHA256SUMS.
# O gateway só existe em x86_64 porque o llama-server não está compilado para ARM.
set -euo pipefail
export MSYS_NO_PATHCONV=1
OUT=${1:-dist/release}
IMAGE=${IMAGE:-ai-cluster:local}
ARM_TARGET=${ARM_TARGET:-armv7-unknown-linux-musleabihf}
HERE=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$OUT"
rm -f "$OUT"/*.tar.gz "$OUT"/SHA256SUMS "$OUT"/VERSION "$OUT"/install.sh   # sem restos de builds antigos
STAGE="$OUT/.stage-$$"; mkdir -p "$STAGE"
CID=""
trap 'rm -rf "$STAGE"; [ -z "$CID" ] || docker rm -f "$CID" >/dev/null 2>&1' EXIT

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$HERE/crates/controller/Cargo.toml" | head -n 1)
[ -n "$VERSION" ] || { echo "não achei a versão em crates/controller/Cargo.toml" >&2; exit 1; }

# pack <nome> <arquivo>... : junta os arquivos de $STAGE/<nome> num .tar.gz sem diretório raiz.
pack() {
  name=$1
  tar --mode=755 --owner=0 --group=0 -C "$STAGE/$name" -czf "$OUT/$name.tar.gz" .
  echo "  $name.tar.gz ($(du -h "$OUT/$name.tar.gz" | cut -f1))"
}

CID=$(docker create "$IMAGE")
x86() { mkdir -p "$STAGE/$1"; shift; for b in "$@"; do docker cp "$CID:/usr/local/bin/$b" "$STAGE/$name/$b"; done; }

name=controller-linux-x86_64; x86 $name ai-cluster-controller aic; pack $name
name=gateway-linux-x86_64;    x86 $name ai-cluster-gateway llama-server aic; pack $name
name=worker-linux-x86_64;     x86 $name ai-cluster-worker ggml-rpc-server aic-wait aic; pack $name

# armv7: binários do build-armv7.sh (volume aic-target-armv7) e o rpc-server do build.sh do llama.cpp.
arm() { # arm <nome> <volume> <caminho dentro dele> <binário>...
  mkdir -p "$STAGE/$1"; vol=$2; dir=$3; shift 3
  for b in "$@"; do
    docker run --rm -v "$vol:/v:ro" busybox cat "/v/$dir/$b" > "$STAGE/$name/$b"
  done
}
name=controller-linux-armv7
arm $name aic-target-armv7 "$ARM_TARGET/release" ai-cluster-controller aic; pack $name
name=worker-linux-armv7
arm $name aic-target-armv7 "$ARM_TARGET/release" ai-cluster-worker aic-wait aic
docker run --rm -v aic-llama:/v:ro busybox cat /v/llama.cpp/build-armv7m/bin/ggml-rpc-server \
  > "$STAGE/$name/ggml-rpc-server"
pack $name

# O que não depende de arquitetura: o gerenciador pós-instalação e o bootstrap do worker.
mkdir -p "$STAGE/tools"
cp "$HERE/os/setup" "$HERE/os/bootstrap.sh" "$STAGE/tools/"
sed -i 's/\r$//' "$STAGE/tools/setup" "$STAGE/tools/bootstrap.sh"
pack tools

cp "$HERE/os/install.sh" "$OUT/install.sh"
sed -i 's/\r$//' "$OUT/install.sh"
printf '%s\n' "$VERSION" > "$OUT/VERSION"
(cd "$OUT" && rm -f SHA256SUMS && for f in install.sh VERSION *.tar.gz; do printf '%s  %s
' "$(sha256sum "$f" | cut -d' ' -f1)" "$f"; done > SHA256SUMS)
echo "release v$VERSION em $OUT"
cat "$OUT/SHA256SUMS"
