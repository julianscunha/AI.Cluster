#!/usr/bin/env bash
# Fatia da Fase G1: quarentena automática por resultado inválido.
#
# Critério verificado: um worker que devolve checksum errado tem os resultados rejeitados,
# é posto em QUARANTINED depois de 3 em seguida, e o trabalho termina inteiro no worker honesto.
# AI_CLUSTER_CORRUPT_RESULTS=1 faz o worker mentir (gancho de teste).
#
# Sequência:
#   A) sozinho, o mentiroso aparece como QUARANTINED na tabela `nodes`;
#   B) um worker honesto entra e as 6 unidades completam (as rejeitadas voltaram à fila);
#   C) o evento node_quarantined foi registrado.
set -uo pipefail

NET=aic-net-g1
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller-g1:50051
ENVS="-e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1"

run() {
  MSYS_NO_PATHCONV=1 docker run $ENVS --rm --network "$NET" -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

teardown() {
  docker rm -f aic-controller-g1 aic-worker-liar aic-worker-honest aic-cli-bench >/dev/null 2>&1
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

MSYS_NO_PATHCONV=1 docker run $ENVS -d --name aic-controller-g1 --network "$NET" -v aic-target:/t \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null

start_worker() {
  local name=$1 node=$2; shift 2
  MSYS_NO_PATHCONV=1 docker run $ENVS -d --name "$name" --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID="$node" "$@" \
    "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
}

# Só o mentiroso no começo: sozinho ele recebe unidades até acumular as 3 rejeições seguidas.
# Com um honesto ao lado a corrida decidiria quantas unidades o mentiroso chega a ver.
start_worker aic-worker-liar node-liar -e AI_CLUSTER_CORRUPT_RESULTS=1
sleep 6
MSYS_NO_PATHCONV=1 docker run $ENVS -d --name aic-cli-bench --network "$NET" -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER   "$IMG" sh -c "$R/ai-cluster benchmark --size 64 --repetitions 6" >/dev/null
sleep 8

echo "=== A) o mentiroso está em quarentena ==="
NODES=$(run "$R/ai-cluster nodes")
echo "$NODES"
STATE=$(echo "$NODES" | awk '$1 == "node-liar" { print $2 }')
if [ "$STATE" = "QUARANTINED" ]; then
  echo "OK: node-liar QUARANTINED"
else
  echo "FALHA: esperado QUARANTINED, veio '$STATE'"
  teardown
  exit 1
fi

echo "=== B) um worker honesto entra e o trabalho termina inteiro ==="
start_worker aic-worker-honest node-honest
sleep 15
BENCH=$(docker logs aic-cli-bench 2>&1)
echo "$BENCH" | tail -3
if echo "$BENCH" | grep -q "concluidas=6 pendentes=0 falharam=0"; then
  echo "OK: 6 unidades concluídas, nenhuma falhou"
else
  echo "FALHA: esperado concluidas=6 pendentes=0 falharam=0"
  teardown
  exit 1
fi

echo "=== C) o evento node_quarantined foi registrado ==="
if run "$R/ai-cluster events --limit 200" | grep -q node_quarantined; then
  echo "OK: node_quarantined"
else
  echo "FALHA: sem evento node_quarantined"
  teardown
  exit 1
fi

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== fim ==="
