#!/usr/bin/env bash
# Gera o par Ed25519 das releases em ~/.aic (uma vez). A pública entra na imagem
# (build-rootfs.sh a copia para /etc/aic-release.pub); a privada nunca sai da máquina do operador.
# Trocar a chave exige uma imagem nova com a pública nova (rotação ainda não existe).
set -euo pipefail
DIR=${AIC_KEYS:-$HOME/.aic}
mkdir -p "$DIR"; chmod 700 "$DIR"
[ -f "$DIR/release.key" ] && { echo "já existe $DIR/release.key" >&2; exit 0; }
openssl genpkey -algorithm ed25519 -out "$DIR/release.key"; chmod 600 "$DIR/release.key"
openssl pkey -in "$DIR/release.key" -pubout -out "$DIR/release.pub"
echo "chave gerada em $DIR"
