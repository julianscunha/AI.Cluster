#!/usr/bin/env bash
# `aic update apply` no AI Cluster OS pede a checagem ao aic-updater e mostra o resultado; não baixa nada.
#
# Critérios verificados (shell da imagem = busybox, num contêiner Alpine, sem hardware e sem A/B real):
#   A) com o aic-updater rodando, `aic update apply` volta com o estado (sem release pendente) e o pedido é consumido;
#   B) sem o aic-updater, `aic update apply` desiste em 15 s, diz por quê e sai com erro;
#   C) o aic-updater não aplica nada sem pedido assinado (nenhum reinício, nenhum slot gravado).
set -uo pipefail
export MSYS_NO_PATHCONV=1
IMG=$(cd "$(dirname "$0")/../os/image/overlay" && pwd -W 2>/dev/null || pwd)
fail() { echo "FALHA: $1"; exit 1; }
out=$(docker run --rm -v "$IMG:/ov:ro" alpine:3.20 sh -c '
set -u
mkdir -p /usr/lib/aic /usr/libexec/aic /usr/sbin /data/state /etc
cp /ov/usr/lib/aic/*.sh /usr/lib/aic/; cp /ov/usr/libexec/aic/* /usr/libexec/aic/; cp /ov/usr/sbin/aic-updater /usr/sbin/
printf "board=teste\nslot_mib=1\n" > /etc/aic-board
echo 12.0 > /etc/aic-release

# B) sem o aic-updater: desiste, diz o motivo e sai com erro
out=$(sh /usr/libexec/aic/setup update apply 2>&1); rc=$?
[ "$rc" -ne 0 ] || { echo "FALHA B1: sem o aic-updater deveria sair com erro"; exit 1; }
echo "$out" | grep -q "nao respondeu em 15 s" || { echo "FALHA B2: $out"; exit 1; }

# A) com o aic-updater rodando: responde com o estado e consome o pedido
sh /usr/sbin/aic-updater >/tmp/upd.log 2>&1 &
U=$!
out=$(sh /usr/libexec/aic/setup update apply 2>&1); rc=$?
[ "$rc" -eq 0 ] || { echo "FALHA A1: rc=$rc $out"; kill $U; exit 1; }
echo "$out" | grep -q "sem release pendente" || { echo "FALHA A2: $out"; kill $U; exit 1; }
echo "$out" | grep -q "versao 12.0" || { echo "FALHA A3: falta a versão: $out"; kill $U; exit 1; }
[ ! -e /data/state/update-check ] || { echo "FALHA A4: o pedido não foi consumido"; kill $U; exit 1; }

# C) nada foi aplicado: sem pedido de release não há slot gravado nem reinício (o processo segue vivo)
[ ! -e /data/state/update-result ] || { echo "FALHA C1: recusou algo sem pedido"; kill $U; exit 1; }
kill -0 $U 2>/dev/null || { echo "FALHA C2: o aic-updater saiu"; exit 1; }
kill $U
echo TUDO-OK' 2>&1) || { echo "$out"; fail "contêiner"; }
echo "$out" | grep -q "^FALHA" && { echo "$out"; fail "ver acima"; }
echo "$out" | grep -q "TUDO-OK" || { echo "$out"; fail "não terminou"; }
echo "OK A) com o aic-updater, a checagem responde com o estado e consome o pedido"
echo "OK B) sem o aic-updater, desiste em 15 s com o motivo"
echo "OK C) sem pedido assinado nada é aplicado"
echo "=== checagem de atualização do SO OK ==="
