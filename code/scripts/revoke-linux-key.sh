#!/usr/bin/env bash
# Revogação de chaves de Linux comum (ADR-031, item 7): grava revoked.txt (uma impressão de 32 hex por linha) e
# revoked.txt.sig, assinada pela chave que continua válida (a rotacionada). package-release.sh os leva na release
# (AIC_LINUX_REVOCATION=<pasta>); as máquinas aplicam a lista junto com a atualização e nunca mais aceitam a chave.
#
#   bash code/scripts/revoke-linux-key.sh ~/.aic/linux2.key ~/.aic/revogacao-linux ~/.aic/linux.pub [outra.pub ...]
#   AIC_LINUX_ROTATION=~/.aic/rotacao-linux AIC_LINUX_REVOCATION=~/.aic/revogacao-linux bash code/scripts/package-release.sh <saída>
#
# A lista tem de ser assinada pela chave nova (a que a release rotaciona, ou a já pinada depois da rotação), nunca pela
# revogada. Quem só tem a chave pinada comprometida e nenhuma outra só se recupera reinstalando.
set -euo pipefail
export MSYS_NO_PATHCONV=1
KEY=${1:?uso: revoke-linux-key.sh <chave-privada-que-assina> <pasta-de-saída> <pública-revogada.pub>...}
OUT=${2:?pasta de saída}; shift 2
[ $# -ge 1 ] || { echo "informe ao menos uma pública a revogar" >&2; exit 2; }
win() { cygpath -m "$1" 2>/dev/null || echo "$1"; }
mkdir -p "$OUT"; : > "$OUT/revoked.txt"
for pub in "$@"; do
  openssl pkey -pubin -in "$(win "$pub")" -text_pub -noout 2>/dev/null | grep -q '^ED25519' || { echo "$pub não é uma pública Ed25519" >&2; exit 2; }
  sha256sum "$pub" | cut -c1-32 >> "$OUT/revoked.txt"
done
SIG=$(mktemp); trap 'rm -f "$SIG"' EXIT
openssl pkeyutl -sign -inkey "$(win "$KEY")" -rawin -in "$(win "$OUT/revoked.txt")" -out "$(win "$SIG")"
printf '%s' "$(base64 -w0 < "$SIG")" > "$OUT/revoked.txt.sig"
echo "revogação gravada em $OUT: $(wc -l < "$OUT/revoked.txt" | tr -d ' ') chave(s)"
