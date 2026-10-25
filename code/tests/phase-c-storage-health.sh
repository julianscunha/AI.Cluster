#!/usr/bin/env bash
# Fatia da Fase C2/C4: saúde de storage entra no score de saúde, e o placement a usa.
#
# Critério verificado: um nó com o disco degradado reporta saúde pior que um nó com o disco
# como no registro, com a mesma capability, e a unidade vai para o nó saudável.
# A degradação entra por AI_CLUSTER_STORAGE_HEALTH (1.0 = como no registro, 0.2 = 5x mais
# lento) porque degradar um disco de verdade não é reprodutível numa máquina de dev.
#
# Sequência:
#   A) a saúde de storage de cada nó atravessa worker → proto → controller → CLI;
#   B) o score de saúde do nó ok é maior que o do nó lento;
#   C) o placement manda a unidade para o nó ok.
set -uo pipefail

NET=aic-net-c5
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller-c5:50051

run() {
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

teardown() {
  docker rm -f aic-controller-c5 aic-worker-ok aic-worker-slow >/dev/null 2>&1
}

# Reads one column of one node's row from the `nodes` table. Columns are, in order:
# NÓ ESTADO ARQ THREADS RAM_GB COMPUTE SAÚDE CPU_% TEMP_C SWAP_% ARMAZ — so SAÚDE is 7
# and TEMP_C is 9. Padded columns mean awk's whitespace split lands on the same numbers.
field() {
  echo "$1" | awk -v node="$2" -v col="$3" '$1 == node { print $col }'
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-controller-c5 --network "$NET" -v aic-target:/t \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null

start_worker() {
  local name=$1 node=$2 health=$3
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name "$name" --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID="$node" \
    -e AI_CLUSTER_CPU_THREADS=2 -e AI_CLUSTER_MEMORY_BYTES=1073741824 \
    -e AI_CLUSTER_COMPUTE_SCORE=1.0 -e AI_CLUSTER_STORAGE_HEALTH="$health" \
    "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
}

start_worker aic-worker-ok node-ok 1.0
start_worker aic-worker-slow node-slow 0.2
# The health score rides the heartbeat (5 s), so the first report needs a moment — and the
# placement leg must only run once both scores are in the registry, or a node with no health
# would tie on cost and the unit could land anywhere.
sleep 12

NODES=$(run "$R/ai-cluster nodes")
echo "$NODES"

echo "=== A) a saúde de storage atravessa worker → proto → controller → CLI ==="
OK_S=$(run "$R/ai-cluster node show node-ok" | grep -o 'armaz_saude=[^ ]*')
SLOW_S=$(run "$R/ai-cluster node show node-slow" | grep -o 'armaz_saude=[^ ]*')
if [ "$OK_S" = "armaz_saude=1.00" ] && [ "$SLOW_S" = "armaz_saude=0.20" ]; then
  echo "OK: $OK_S / $SLOW_S"
else
  echo "FALHA: esperado armaz_saude=1.00 / 0.20, veio '$OK_S' / '$SLOW_S'"
  teardown
  exit 1
fi

echo "=== B) o score de saúde do nó ok é maior ==="
OK_H=$(field "$NODES" node-ok 7)
SLOW_H=$(field "$NODES" node-slow 7)
if awk -v a="$OK_H" -v b="$SLOW_H" 'BEGIN { exit !(a > b) }'; then
  echo "OK: SAÚDE ok=$OK_H > lento=$SLOW_H"
else
  echo "FALHA: esperado SAÚDE(ok) > SAÚDE(lento), veio $OK_H vs $SLOW_H"
  echo "      se os dois estão no piso, a memória ou o swap do host está saturado —"
  echo "      o termo de storage não tem como aparecer, e o ambiente não serve para este teste"
  teardown
  exit 1
fi

echo "=== C) o placement manda a unidade para o nó ok ==="
BENCH=$(run "$R/ai-cluster benchmark --size 64 --repetitions 1")
echo "$BENCH"
# The per-unit rows start with the work id; the second field is the node that ran it.
RAN=$(echo "$BENCH" | awk '$1 ~ /^w-/ { print $2; exit }')
if [ "$RAN" = "node-ok" ]; then
  echo "OK: a única unidade rodou em node-ok, o nó mais saudável"
else
  echo "FALHA: esperado node-ok, veio '$RAN'"
  teardown
  exit 1
fi

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== fim ==="
