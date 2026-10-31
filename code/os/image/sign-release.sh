#!/usr/bin/env bash
# Assina uma release (F19) e imprime os argumentos de `aic cluster rollout`.
#
#   bash code/os/image/sign-release.sh <versão> <rootfs.gz> <rootfs.ext4> [placa] [chave]
#
# A chave privada (Ed25519) fica fora do repositório, por padrão em ~/.aic/release.key (gere com
# release-keys.sh). A mensagem assinada é a mesma que o aic-updater monta no worker:
#   aic-release-v1 \n placa \n versão \n sha256(gz) \n sha256(ext4) \n
set -euo pipefail
VERSION=$1; GZ=$2; EXT4=$3; BOARD=${4:-armv7-opipc}; KEY=${5:-$HOME/.aic/release.key}
case "$VERSION$BOARD" in *[!0-9.A-Za-z_-]*) echo "versão/placa com caracteres inválidos" >&2; exit 2;; esac
case "$VERSION" in ""|*[!0-9.]*) echo "versão: só números e pontos" >&2; exit 2;; esac
[ -f "$KEY" ] || { echo "falta a chave $KEY (rode release-keys.sh)" >&2; exit 2; }
SHA=$(sha256sum "$GZ" | cut -d' ' -f1); IMG=$(sha256sum "$EXT4" | cut -d' ' -f1)
# openssl nativo do Windows não lê caminhos /d/…: converte quando há cygpath
win() { cygpath -m "$1" 2>/dev/null || echo "$1"; }
MSG=$(mktemp); trap 'rm -f "$MSG" "$MSG.sig"' EXIT
printf 'aic-release-v1\n%s\n%s\n%s\n%s\n' "$BOARD" "$VERSION" "$SHA" "$IMG" > "$MSG"
openssl pkeyutl -sign -inkey "$(win "$KEY")" -rawin -in "$(win "$MSG")" -out "$(win "$MSG.sig")"
echo "$VERSION --sha256 $SHA --image-sha256 $IMG --signature $(base64 -w0 < "$MSG.sig")"
