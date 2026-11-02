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
# Com a imagem x86 montada (os/image/build: disk-x86.img(.gz) e installer/, de build-installer.sh), sai também
# dedicado.tar.gz (modo dedicado, ADR-023): a imagem, o pendrive e as somas assinadas que o controller serve.
# O gateway só existe em x86_64 porque o llama-server não está compilado para ARM.
# SHA256SUMS.sig é a assinatura Ed25519 (base64) do SHA256SUMS com a chave de Linux comum
# (~/.aic/linux.key, de os/image/release-keys.sh linux). A pública (os/linux.pub) vai no tools.tar.gz
# como release.pub e o instalador a pina em /etc/ai-cluster/release.pub (ADR-025).
# Rotação (ADR-025): AIC_LINUX_ROTATION=<pasta de rotate-linux-key.sh> leva release.pub.next e .sig nesta release
# (só nesta; depois troque os/linux.pub pela chave nova e assine as próximas com ela).
set -euo pipefail
export MSYS_NO_PATHCONV=1
OUT=${1:-dist/release}
IMAGE=${IMAGE:-ai-cluster:local}
ARM_TARGET=${ARM_TARGET:-armv7-unknown-linux-musleabihf}
HERE=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$OUT"
rm -f "$OUT"/*.tar.gz "$OUT"/SHA256SUMS "$OUT"/SHA256SUMS.sig "$OUT"/VERSION "$OUT"/install.sh "$OUT"/release.pub.next* "$OUT"/revoked.txt* # sem restos de builds antigos
STAGE="$OUT/.stage-$$"; mkdir -p "$STAGE"
CID=""
trap 'rm -rf "$STAGE"; [ -z "$CID" ] || docker rm -f "$CID" >/dev/null 2>&1' EXIT

VERSION=$(sh "$HERE/scripts/version.sh") || exit 1   # fonte única (ADR-029)
LINUX_KEY=${AIC_LINUX_KEY:-$HOME/.aic/linux.key}
[ -f "$LINUX_KEY" ] || { echo "falta a chave $LINUX_KEY (rode os/image/release-keys.sh linux)" >&2; exit 2; }
# openssl nativo do Windows não lê caminhos /d/…: converte quando há cygpath
win() { cygpath -m "$1" 2>/dev/null || echo "$1"; }
ROT=${AIC_LINUX_ROTATION:-}
ARQ="install.sh VERSION *.tar.gz"
if [ -n "$ROT" ]; then
  [ -f "$ROT/release.pub.next" ] && [ -f "$ROT/release.pub.next.sig" ] || { echo "faltam release.pub.next e .sig em $ROT (rode rotate-linux-key.sh)" >&2; exit 2; }
  # Só sai rotação que confere com a chave que o repositório pina: uma rotação antiga não pode ir numa release nova.
  SIGR=$(mktemp); base64 -d < "$ROT/release.pub.next.sig" > "$SIGR" 2>/dev/null || : > "$SIGR"
  openssl pkeyutl -verify -pubin -inkey "$(win "$HERE/os/linux.pub")" -rawin -in "$(win "$ROT/release.pub.next")" -sigfile "$(win "$SIGR")" >/dev/null 2>&1 \
    || { rm -f "$SIGR"; echo "a rotação em $ROT não confere com os/linux.pub: gere de novo com rotate-linux-key.sh (ou apague a pasta)" >&2; exit 2; }
  rm -f "$SIGR"
  cp "$ROT/release.pub.next" "$ROT/release.pub.next.sig" "$OUT/"
  ARQ="$ARQ release.pub.next release.pub.next.sig"
fi

# Revogação (ADR-031, item 7): AIC_LINUX_REVOCATION=<pasta de revoke-linux-key.sh> leva revoked.txt e .sig. A lista tem
# de ser assinada pela chave nova (a da rotação desta release, ou a de os/linux.pub); senão não sai.
REV=${AIC_LINUX_REVOCATION:-}
if [ -n "$REV" ]; then
  [ -f "$REV/revoked.txt" ] && [ -f "$REV/revoked.txt.sig" ] || { echo "faltam revoked.txt e .sig em $REV (rode revoke-linux-key.sh)" >&2; exit 2; }
  SIGV=$(mktemp); base64 -d < "$REV/revoked.txt.sig" > "$SIGV" 2>/dev/null || : > "$SIGV"
  VPUB="$HERE/os/linux.pub"; [ -z "$ROT" ] || VPUB="$ROT/release.pub.next"
  openssl pkeyutl -verify -pubin -inkey "$(win "$VPUB")" -rawin -in "$(win "$REV/revoked.txt")" -sigfile "$(win "$SIGV")" >/dev/null 2>&1     || { rm -f "$SIGV"; echo "a lista em $REV não foi assinada por $VPUB (a chave rotacionada): gere de novo com revoke-linux-key.sh" >&2; exit 2; }
  rm -f "$SIGV"
  cp "$REV/revoked.txt" "$REV/revoked.txt.sig" "$OUT/"
  ARQ="$ARQ revoked.txt revoked.txt.sig"
fi

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

# Modo dedicado (ADR-023): com o instalador x86 e a imagem montados (build-installer.sh, mk-disk.sh), a release
# leva dedicado.tar.gz: a imagem que o controller serve no convite, o pendrive (usb.zip) e o instalador. Sem eles,
# a release sai como antes. Assinado com a mesma chave de Linux; o controller confere antes de servir.
DEDI=$HERE/os/image/build
if [ -f "$DEDI/installer/BOOTX64.EFI" ] && [ -f "$DEDI/installer/vmlinuz" ] && [ -f "$DEDI/installer/initramfs.gz" ] \
  && { [ -f "$DEDI/disk-x86.img.gz" ] || [ -f "$DEDI/disk-x86.img" ]; }; then
  D="$STAGE/dedicado"; mkdir -p "$D"
  if [ -f "$DEDI/disk-x86.img.gz" ]; then cp "$DEDI/disk-x86.img.gz" "$D/"; else gzip -1 -c "$DEDI/disk-x86.img" > "$D/disk-x86.img.gz"; fi
  cp "$DEDI/installer/BOOTX64.EFI" "$DEDI/installer/vmlinuz" "$DEDI/installer/initramfs.gz" "$D/"
  cp "$HERE/os/mkusb.py" "$D/"
  printf '%s\n' "$VERSION" > "$D/VERSION"
  (cd "$D" && for f in disk-x86.img.gz VERSION BOOTX64.EFI vmlinuz initramfs.gz mkusb.py; do
    printf '%s  %s\n' "$(sha256sum "$f" | cut -d' ' -f1)" "$f"; done > SHA256SUMS)
  SIG_D=$(mktemp)
  openssl pkeyutl -sign -inkey "$(win "$LINUX_KEY")" -rawin -in "$(win "$D/SHA256SUMS")" -out "$(win "$SIG_D")"
  base64 -w0 < "$SIG_D" > "$D/SHA256SUMS.sig"; rm -f "$SIG_D"
  pack dedicado
fi

cp "$HERE/os/install.sh" "$OUT/install.sh"
sed -i 's/\r$//' "$OUT/install.sh"
printf '%s\n' "$VERSION" > "$OUT/VERSION"
(cd "$OUT" && rm -f SHA256SUMS && for f in $ARQ; do printf '%s  %s
' "$(sha256sum "$f" | cut -d' ' -f1)" "$f"; done > SHA256SUMS)
# Assinatura Ed25519 bruta (-rawin) do SHA256SUMS, em base64 numa linha: o formato que o rollout pede.
SIG_BIN=$(mktemp); trap 'rm -rf "$STAGE" "$SIG_BIN"; [ -z "$CID" ] || docker rm -f "$CID" >/dev/null 2>&1' EXIT
openssl pkeyutl -sign -inkey "$(win "$LINUX_KEY")" -rawin -in "$(win "$OUT/SHA256SUMS")" -out "$(win "$SIG_BIN")"
printf '%s' "$(base64 -w0 < "$SIG_BIN")" > "$OUT/SHA256SUMS.sig"
echo "release v$VERSION em $OUT"
cat "$OUT/SHA256SUMS"
echo "SHA256SUMS.sig assinado com $LINUX_KEY"
echo "impressão da chave de Linux (publique no aviso da release; o convite e o install.sh --key-fingerprint a usam): $(sha256sum "$HERE/os/linux.pub" | cut -c1-32)"
echo "rollout dos workers Linux: aic cluster rollout $VERSION --linux-signature $(cat "$OUT/SHA256SUMS.sig")"
