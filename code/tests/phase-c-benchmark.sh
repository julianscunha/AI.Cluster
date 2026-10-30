#!/usr/bin/env bash
# Fatia da Fase C (C2): benchmark de capability — o worker mede o próprio compute.
#
# Critério verificado: adicionar um nó produz um capability profile com o compute
# medido, não apenas a contagem de threads. Antes desta fatia o worker reportava
# compute_score=0 e o modelo de placement caía na contagem de threads; agora ele
# mede o matmul no próprio kernel que executa as unidades e reporta a taxa por core.
#
# Sequência:
#   A) com benchmark: o nó reporta COMPUTE > 0, coerente entre duas leituras (reproduzível).
#   B) com AI_CLUSTER_COMPUTE_SCORE=2.0: o valor fixado aparece no relatório — prova que
#      a medição é a fonte do número, sem depender da velocidade do host.
#
# A medição é por core: o modelo de custo multiplica pelo número de threads reportado,
# então medir multithread contaria os cores duas vezes. O score 1.0 é a referência
# (~6 GFLOP/s por core neste kernel); um core mais lento mede menos e um mais rápido mais.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051

run() {
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

teardown() {
  docker rm -f aic-controller aic-worker-bench aic-worker-pinned >/dev/null 2>&1
}

# Extracts the COMPUTE column for a node id from the `nodes` table. The table prints the
# node id in the first column and COMPUTE in the sixth, so the match is by node id.
compute_of() {
  local node=$1
  awk -v node="$node" '$1 == node { print $6 }'
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

echo "=== A) benchmark medido: o nó reporta o compute que mediu ==="
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-controller --network "$NET" -v aic-target:/t \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-worker-bench --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-bench \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 8

NODES=$(run "$R/aic cluster nodes")
echo "$NODES"
SCORE=$(echo "$NODES" | compute_of node-bench)
echo "compute_score medido = $SCORE"
if awk -v s="$SCORE" 'BEGIN { exit !(s+0 > 0) }'; then
  echo "OK: o worker reportou um compute medido, não o fallback 0"
else
  echo "FALHA: compute_score deveria ser > 0 (veio '$SCORE')"
  exit 1
fi

# Reproduzível: uma segunda leitura do mesmo nó não pode mudar o perfil reportado.
SCORE2=$(run "$R/aic cluster nodes" | compute_of node-bench)
echo "compute_score relido  = $SCORE2"
if [ "$SCORE" = "$SCORE2" ]; then
  echo "OK: o perfil é reproduzível entre leituras"
else
  echo "FALHA: o perfil mudou entre leituras ('$SCORE' -> '$SCORE2')"
  exit 1
fi

teardown
echo "=== B) score fixado: AI_CLUSTER_COMPUTE_SCORE=2.0 é o número reportado ==="
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-controller --network "$NET" -v aic-target:/t \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-worker-pinned --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-pinned \
  -e AI_CLUSTER_COMPUTE_SCORE=2.0 \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 8

PINNED=$(run "$R/aic cluster nodes" | compute_of node-pinned)
echo "compute_score fixado = $PINNED"
if [ "$PINNED" = "2.00" ]; then
  echo "OK: o score fixado foi reportado como veio"
else
  echo "FALHA: esperado 2.00, veio '$PINNED'"
  exit 1
fi

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== fim ==="
