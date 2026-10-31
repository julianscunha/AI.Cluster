#!/usr/bin/env bash
# Gera os arquivos de uma release, um por componente e arquitetura, com nomes SEM versão para o
# instalador achar a última release em releases/latest/download/<arquivo>.
#
#   docker compose -f deploy/docker-compose.yml build                 # imagem ai-cluster:local (x86_64)
#   TARGET=armv7-unknown-linux-musleabihf bash code/scripts/build-armv7.sh   # binários armv7
#   bash code/scripts/package-release.sh [saida]                      # padrão: dist/release
#
# Saída: install.sh, VERSION, tools.tar.gz, controller-linux-{x86_64,armv7}.tar.gz,
# gateway-linux-x86_64.tar.gz, worker-linux-{x86_64,armv7}.tar.gz, SHA256SUMS e SHA256SUMS.sig.
# O gateway só existe em x86_64 porque o llama-server não está compilado para ARM.
# SHA256SUMS.sig é a assinatura Ed25519 (base64) do SHA256SUMS com a chave de Linux comum
# (~/.aic/linux.key, de os/image/release-keys.sh linux). A pública (os/linux.pub) vai no tools.tar.gz
# como release.pub e o instalador a pina em /etc/ai-cluster/release.pub (ADR-025).
set -euo pipefail
export MSYS_NO_PATHCONV=1
OUT=${1:-dist/release}
IMAGE=${IMAGE:-ai-cluster:local}
ARM_TARGET=${ARM_TARGET:-armv7-unknown-linux-musleabihf}
HERE=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$OUT"
rm -f "$OUT"/*.tar.gz "$OUT"/SHA256SUMS "$OUT"/SHA256SUMS.sig "$OUT"/VERSION "$OUT"/install.sh   # sem restos de builds antigos
STAGE="$OUT/.stage-$$"; mkdir -p "$STAGE"
CID=""
trap 'rm -rf "$STAGE"; [ -z "$CID" ] || docker rm -f "$CID" >/dev/null 2>&1' EXIT

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$HERE/crates/controller/Cargo.toml" | head -n 1)
[ -n "$VERSION" ] || { echo "não achei a versão em crates/controller/Cargo.toml" >&2; exit 1; }
LINUX_KEY=${AIC_LINUX_KEY:-$HOME/.aic/linux.key}
[ -f "$LINUX_KEY" ] || { echo "falta a chave $LINUX_KEY (rode os/image/release-keys.sh linux)" >&2; exit 2; }
# openssl nativo do Windows não lê caminhos /d/…: converte quando há cygpath
win() { cygpath -m "$1" 2>/dev/null || echo "$1"; }

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
# O setup desta versão e o comando de atualização que o worker Linux chama (ADR-025).
extras_worker() {
  cp "$HERE/os/setup" "$HERE/os/aic-update-linux" "$STAGE/$1/"
  sed -i 's/\r$//' "$STAGE/$1/setup" "$STAGE/$1/aic-update-linux"
  chmod 755 "$STAGE/$1/setup" "$STAGE/$1/aic-update-linux"
}
name=worker-linux-x86_64;     x86 $name ai-cluster-worker ggml-rpc-server aic-wait aic; extras_worker $name; pack $name

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
extras_worker $name
pack $name

# O que não depende de arquitetura: o gerenciador pós-instalação e o bootstrap do worker.
mkdir -p "$STAGE/tools"
cp "$HERE/os/setup" "$HERE/os/bootstrap.sh" "$STAGE/tools/"
cp "$HERE/os/linux.pub" "$STAGE/tools/release.pub"   # a chave que o instalador pina (ADR-025)
sed -i 's/\r$//' "$STAGE/tools/setup" "$STAGE/tools/bootstrap.sh"
pack tools

cp "$HERE/os/install.sh" "$OUT/install.sh"
sed -i 's/\r$//' "$OUT/install.sh"
printf '%s\n' "$VERSION" > "$OUT/VERSION"
(cd "$OUT" && rm -f SHA256SUMS && for f in install.sh VERSION *.tar.gz; do printf '%s  %s
' "$(sha256sum "$f" | cut -d' ' -f1)" "$f"; done > SHA256SUMS)
# Assinatura Ed25519 bruta (-rawin) do SHA256SUMS, em base64 numa linha: o formato que o rollout pede.
SIG_BIN=$(mktemp); trap 'rm -rf "$STAGE" "$SIG_BIN"; [ -z "$CID" ] || docker rm -f "$CID" >/dev/null 2>&1' EXIT
openssl pkeyutl -sign -inkey "$(win "$LINUX_KEY")" -rawin -in "$(win "$OUT/SHA256SUMS")" -out "$(win "$SIG_BIN")"
printf '%s' "$(base64 -w0 < "$SIG_BIN")" > "$OUT/SHA256SUMS.sig"
echo "release v$VERSION em $OUT"
cat "$OUT/SHA256SUMS"
echo "SHA256SUMS.sig assinado com $LINUX_KEY"
echo "rollout dos workers Linux: aic cluster rollout $VERSION --linux-signature $(cat "$OUT/SHA256SUMS.sig")"
