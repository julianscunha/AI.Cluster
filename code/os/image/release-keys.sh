#!/usr/bin/env bash
# Gera o par Ed25519 das releases em ~/.aic (uma vez). A pública entra na imagem
# (build-rootfs.sh a copia para /etc/aic-release.d/); a privada nunca sai da máquina do operador.
# Rotação: `release-keys.sh nova` gera nova.key/nova.pub; construa a imagem com
# AIC_PUBS="~/.aic/release.pub ~/.aic/nova.pub" e assine com a nova. A imagem seguinte, só com a
# nova pública, revoga a antiga (a versão só cresce, então a antiga não volta).
set -euo pipefail
DIR=${AIC_KEYS:-$HOME/.aic}
N=${1:-release}
case "$N" in *[!A-Za-z0-9_-]*) echo "nome inválido" >&2; exit 2;; esac
mkdir -p "$DIR"; chmod 700 "$DIR"
[ -f "$DIR/$N.key" ] && { echo "já existe $DIR/$N.key" >&2; exit 0; }
openssl genpkey -algorithm ed25519 -out "$DIR/$N.key"; chmod 600 "$DIR/$N.key"
openssl pkey -in "$DIR/$N.key" -pubout -out "$DIR/$N.pub"
echo "chave gerada em $DIR"
