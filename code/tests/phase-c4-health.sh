#!/usr/bin/env bash
# Fatia da Fase C4: o score de saúde leva a pressão térmica, e o placement o usa.
#
# Critério verificado: um nó quente reporta score de saúde pior que um nó frio com a mesma
# capability, e o escalonador manda a unidade para o nó mais saudável.
#
# Os dois nós reportam a *mesma* capability (threads, memória e compute fixados), então os
# termos de compute, transferência e fila do modelo de custo se cancelam e a única coisa que
# separa os dois é o termo de saúde. A temperatura entra por AI_CLUSTER_TEMPERATURE_C porque
# contêiner comum não expõe /sys/class/thermal — sem o override o termo térmico não pode ser
# exercitado nesta máquina.
#
# Sequência:
#   A) a temperatura de cada nó atravessa worker → proto → controller → CLI;
#   B) o score de saúde do nó frio é maior que o do nó quente;
#   C) o placement manda a unidade para o nó frio.
set -uo pipefail

NET=aic-net-c4
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller-c4:50051

run() {
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

teardown() {
  docker rm -f aic-controller-c4 aic-worker-cool aic-worker-hot >/dev/null 2>&1
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

MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-controller-c4 --network "$NET" -v aic-target:/t \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null

start_worker() {
  local name=$1 node=$2 temp=$3
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name "$name" --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID="$node" \
    -e AI_CLUSTER_CPU_THREADS=2 -e AI_CLUSTER_MEMORY_BYTES=1073741824 \
    -e AI_CLUSTER_COMPUTE_SCORE=1.0 -e AI_CLUSTER_TEMPERATURE_C="$temp" \
    "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
}

start_worker aic-worker-cool node-cool 45
start_worker aic-worker-hot node-hot 100
# The health score rides the heartbeat (5 s), so the first report needs a moment — and the
# placement leg must only run once both scores are in the registry, or a node with no health
# would tie on cost and the unit could land anywhere.
sleep 12

NODES=$(run "$R/ai-cluster nodes")
echo "$NODES"

echo "=== A) a temperatura atravessa worker → proto → controller → CLI ==="
COOL_T=$(field "$NODES" node-cool 9)
HOT_T=$(field "$NODES" node-hot 9)
if [ "$COOL_T" = "45.00" ] && [ "$HOT_T" = "100.00" ]; then
  echo "OK: TEMP_C frio=$COOL_T quente=$HOT_T"
else
  echo "FALHA: esperado TEMP_C 45.00 / 100.00, veio '$COOL_T' / '$HOT_T'"
  teardown
  exit 1
fi

echo "=== B) o score de saúde do nó frio é maior ==="
COOL_H=$(field "$NODES" node-cool 7)
HOT_H=$(field "$NODES" node-hot 7)
if awk -v a="$COOL_H" -v b="$HOT_H" 'BEGIN { exit !(a > b) }'; then
  echo "OK: SAÚDE frio=$COOL_H > quente=$HOT_H"
else
  echo "FALHA: esperado SAÚDE(frio) > SAÚDE(quente), veio $COOL_H vs $HOT_H"
  echo "      se os dois estão no piso, a memória ou o swap do host está saturado —"
  echo "      o termo térmico não tem como aparecer, e o ambiente não serve para este teste"
  teardown
  exit 1
fi

echo "=== C) o placement manda a unidade para o nó frio ==="
BENCH=$(run "$R/ai-cluster benchmark --size 64 --repetitions 1")
echo "$BENCH"
# The per-unit rows start with the work id; the second field is the node that ran it.
RAN=$(echo "$BENCH" | awk '$1 ~ /^w-/ { print $2; exit }')
if [ "$RAN" = "node-cool" ]; then
  echo "OK: a única unidade rodou em node-cool, o nó mais saudável"
else
  echo "FALHA: esperado node-cool, veio '$RAN'"
  teardown
  exit 1
fi

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== fim ==="
