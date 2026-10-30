#!/bin/sh
# Teste de bancada do upgrade e rollback (F19), para rodar NA máquina de destino, como root, num
# Linux com systemd e com um controller alcançável. Instala/atualiza o worker de verdade.
#
#   sh test-upgrade.sh <bootstrap.sh> <binário-bom> <controller> <token-de-join> <node-id>
#
#   A) atualizar com um binário bom deixa o serviço ativo e registrado;
#   B) atualizar com um binário quebrado sai com 3 e deixa o binário bom em uso, de novo ativo
#      e registrado (rollback).
set -eu

BOOT=$1; GOOD=$2; CONTROLLER=$3; TOKEN=$4; NODE=$5
PREFIX=/opt/ai-cluster
BAD=$(mktemp)
printf '#!/bin/sh\nexit 1\n' > "$BAD"
chmod +x "$BAD"
trap 'rm -f "$BAD"' EXIT

fail() { echo "FALHA: $1"; exit 1; }
args="--controller $CONTROLLER --token $TOKEN --node-id $NODE --insecure --health-timeout 40"

echo "=== A) atualização com binário bom ==="
sh "$BOOT" $args --binary "$GOOD" >/dev/null || fail "a atualização boa deveria sair com 0"
systemctl is-active --quiet ai-cluster-worker || fail "o serviço deveria estar ativo"
GOOD_SUM=$(sha256sum "$PREFIX/bin/ai-cluster-worker" | cut -d' ' -f1)
echo "OK: ativo e registrado, versão $GOOD_SUM"

echo "=== B) atualização com binário quebrado volta à anterior ==="
SINCE_B=$(date '+%Y-%m-%d %H:%M:%S')
set +e
sh "$BOOT" $args --binary "$BAD" >/tmp/upgrade-bad.log 2>&1
CODE=$?
set -e
[ "$CODE" = 3 ] || { cat /tmp/upgrade-bad.log; fail "esperado código 3, veio $CODE"; }
grep -q "rollback feito" /tmp/upgrade-bad.log || fail "faltou a mensagem de rollback"
NOW_SUM=$(sha256sum "$PREFIX/bin/ai-cluster-worker" | cut -d' ' -f1)
[ "$NOW_SUM" = "$GOOD_SUM" ] || fail "o binário em uso deveria ser o bom depois do rollback"
systemctl is-active --quiet ai-cluster-worker || fail "o serviço deveria estar ativo depois do rollback"
# Registrar leva ~20 s (o worker mede a máquina antes): espera com limite, não um sleep fixo.
waited=0
until journalctl -u ai-cluster-worker --since "$SINCE_B" --no-pager | grep -q "registrado node_id=$NODE aceito=true"; do
  [ "$waited" -lt 90 ] || fail "o worker deveria ter se registrado de novo depois do rollback"
  sleep 3; waited=$((waited + 3))
done
echo "OK: código 3, rollback feito, binário bom em uso, ativo e registrado"
echo "=== fim ==="
