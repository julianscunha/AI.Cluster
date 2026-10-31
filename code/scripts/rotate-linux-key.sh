#!/usr/bin/env bash
# Rotação da chave de Linux comum sem reinstalar (ADR-025): assina a chave nova com a chave atual e grava os dois
# arquivos que package-release.sh leva na release (AIC_LINUX_ROTATION=<pasta>).
#
#   bash code/os/image/release-keys.sh linux2                       # gera ~/.aic/linux2.key e linux2.pub (uma vez)
#   bash code/scripts/rotate-linux-key.sh ~/.aic/linux2.pub ~/.aic/rotacao-linux
#   AIC_LINUX_ROTATION=~/.aic/rotacao-linux bash code/scripts/package-release.sh ../../temp/relNN
#
# Saída: release.pub.next (a pública nova, PEM) e release.pub.next.sig (Ed25519 bruta, base64 numa linha, da
# pública nova). Assina com AIC_LINUX_KEY (padrão ~/.aic/linux.key), a chave que o repositório pina em os/linux.pub.
# Recusa a própria rotação se a assinatura não confere com os/linux.pub.
set -euo pipefail
export MSYS_NO_PATHCONV=1
NOVA=${1:?uso: rotate-linux-key.sh <pública-nova.pub> [pasta-de-saída]}
OUT=${2:-$HOME/.aic/rotacao-linux}
HERE=$(cd "$(dirname "$0")/.." && pwd)
KEY=${AIC_LINUX_KEY:-$HOME/.aic/linux.key}
# openssl nativo do Windows não lê caminhos /d/…: converte quando há cygpath
win() { cygpath -m "$1" 2>/dev/null || echo "$1"; }
[ -f "$KEY" ] || { echo "falta a chave atual $KEY (a que o repositório pina em os/linux.pub)" >&2; exit 2; }
openssl pkey -pubin -in "$(win "$NOVA")" -text_pub -noout 2>/dev/null | grep -q '^ED25519' \
  || { echo "$NOVA não é uma pública Ed25519" >&2; exit 2; }
mkdir -p "$OUT"
cp "$NOVA" "$OUT/release.pub.next"
SIG=$(mktemp); trap 'rm -f "$SIG"' EXIT
openssl pkeyutl -sign -inkey "$(win "$KEY")" -rawin -in "$(win "$OUT/release.pub.next")" -out "$(win "$SIG")"
printf '%s' "$(base64 -w0 < "$SIG")" > "$OUT/release.pub.next.sig"
# Confere com a pinada do repositório antes de entregar: uma rotação que não confere não sai em release nenhuma.
base64 -d < "$OUT/release.pub.next.sig" > "$SIG"
openssl pkeyutl -verify -pubin -inkey "$(win "$HERE/os/linux.pub")" -rawin -in "$(win "$OUT/release.pub.next")" -sigfile "$(win "$SIG")" >/dev/null 2>&1 \
  || { rm -f "$OUT/release.pub.next" "$OUT/release.pub.next.sig"; echo "a assinatura não confere com os/linux.pub: a chave em $KEY não é a pinada" >&2; exit 3; }
echo "rotação gravada em $OUT (release.pub.next e .sig), assinada por $KEY"
echo "próximo passo: AIC_LINUX_ROTATION=$OUT bash code/scripts/package-release.sh <saída>; depois troque os/linux.pub pela chave nova"
