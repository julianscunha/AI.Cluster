#!/usr/bin/env bash
# `aic update status` no AI Cluster OS mostra a atualização que trouxe a versão atual.
#
# Critérios verificados (shell da imagem = busybox, num contêiner Alpine, sem hardware):
#   A) update_record (usr/lib/aic/update.sh, comum ao ARM e ao x86) só grava quando a versão que o worker tentou é a que roda;
#   B) grava uma vez por versão: um boot comum não reescreve a data;
#   C) `aic update status` mostra a última atualização e, à parte, a última recusa;
#   D) sem registro, diz que não há.
set -uo pipefail
export MSYS_NO_PATHCONV=1
IMG=$(cd "$(dirname "$0")/../os/image/overlay" && pwd -W 2>/dev/null || pwd)
fail() { echo "FALHA: $1"; exit 1; }
out=$(docker run --rm -v "$IMG:/ov:ro" alpine:3.20 sh -c '
set -u
mkdir -p /usr/lib/aic /usr/libexec/aic /data/state /etc
cp /ov/usr/lib/aic/*.sh /usr/lib/aic/; cp /ov/usr/libexec/aic/* /usr/libexec/aic/
. /usr/lib/aic/update.sh
echo 12.0 > /etc/aic-release

# D) sem registro nenhum
sh /usr/libexec/aic/setup update status | grep -q "sem registro de atualizacao" || { echo "FALHA D"; exit 1; }

# A) o worker nunca tentou nada: nada a registrar
update_record A; [ ! -e /data/state/update-last ] || { echo "FALHA A1: gravou sem tentativa"; exit 1; }
# tentou outra versão: não é esta atualização
echo 11.9 > /data/state/update-tried; update_record A; [ ! -e /data/state/update-last ] || { echo "FALHA A2: gravou versão diferente"; exit 1; }
# tentou exatamente a que roda: grava
echo 12.0 > /data/state/update-tried; update_record A
grep -q "^ok versao=12.0 slot=A em=" /data/state/update-last || { echo "FALHA A3: nao gravou"; cat /data/state/update-last; exit 1; }

# B) um boot comum não reescreve
first=$(cat /data/state/update-last); sleep 1; update_record A
[ "$(cat /data/state/update-last)" = "$first" ] || { echo "FALHA B: reescreveu a data"; exit 1; }
# uma versão nova grava de novo
echo 12.1 > /etc/aic-release; echo 12.1 > /data/state/update-tried; update_record B
grep -q "versao=12.1 slot=B" /data/state/update-last || { echo "FALHA B2: nao gravou a nova"; exit 1; }

# C) o status mostra a atualização e a recusa
echo "assinatura nao confere" > /data/state/update-result
st=$(sh /usr/libexec/aic/setup update status)
echo "$st" | grep -q "ultima atualizacao: ok versao=12.1 slot=B" || { echo "FALHA C1"; echo "$st"; exit 1; }
echo "$st" | grep -q "ultima recusa:" && echo "$st" | grep -q "assinatura nao confere" || { echo "FALHA C2"; echo "$st"; exit 1; }
echo TUDO-OK' 2>&1) || { echo "$out"; fail "contêiner"; }
echo "$out" | grep -q "^FALHA" && { echo "$out"; fail "ver acima"; }
echo "$out" | grep -q "TUDO-OK" || { echo "$out"; fail "não terminou"; }
echo "OK A) só grava quando a versão tentada é a que roda"
echo "OK B) uma vez por versão"
echo "OK C) aic update status mostra a última atualização e a última recusa"
echo "OK D) sem registro, diz que não há"
echo "=== update status do SO OK ==="
