#!/usr/bin/env bash
# Fatia da Fase G4: classes de prioridade na fila do controller.
#
# Critério verificado: com um só worker e quatro unidades de prioridade 0 já na fila, uma
# unidade de prioridade 5 enfileirada depois é despachada antes de todas elas.
# Sem worker no início, a fila acumula; o worker entra depois e drena na ordem do ranking.
#
# Sequência:
#   A) quatro unidades de prioridade 0, depois uma de prioridade 5, ambas pendentes;
#   B) um worker sobe; a de prioridade 5 é a primeira a completar.
set -uo pipefail

NET=aic-net-g4
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller-g4:50051
ENVS="-e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1"

run() {
  MSYS_NO_PATHCONV=1 docker run $ENVS --rm --network "$NET" -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

teardown() {
  docker rm -f aic-controller-g4 aic-worker-g4 aic-cli-low aic-cli-high >/dev/null 2>&1
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

MSYS_NO_PATHCONV=1 docker run $ENVS -d --name aic-controller-g4 --network "$NET" -v aic-target:/t \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
sleep 2

echo "=== A) quatro unidades de prioridade 0 e uma de prioridade 5, sem worker ==="
MSYS_NO_PATHCONV=1 docker run $ENVS -d --name aic-cli-low --network "$NET" -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER \
  "$IMG" sh -c "$R/aic cluster benchmark --size 64 --repetitions 4 --priority 0 --request-id req-low" >/dev/null
sleep 2
MSYS_NO_PATHCONV=1 docker run $ENVS -d --name aic-cli-high --network "$NET" -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER \
  "$IMG" sh -c "$R/aic cluster benchmark --size 64 --repetitions 1 --priority 5 --request-id req-high" >/dev/null
sleep 2

echo "=== B) um worker sobe; a unidade de prioridade 5 completa primeiro ==="
MSYS_NO_PATHCONV=1 docker run $ENVS -d --name aic-worker-g4 --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-g4 \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 20

# O controller numera as unidades na ordem de enfileiramento: w-0..w-3 são as de prioridade 0
# e w-4 a de prioridade 5. A ordem dos eventos work_completed é a ordem de despacho com um
# só worker.
EVENTS=$(run "$R/aic cluster events --limit 200")
echo "$EVENTS" | grep work_completed
FIRST=$(echo "$EVENTS" | grep work_completed | head -1)
if echo "$FIRST" | grep -q " w-4 "; then
  echo "OK: w-4 (prioridade 5, enfileirada por último) completou primeiro"
else
  echo "FALHA: esperado w-4 primeiro, veio: $FIRST"
  teardown
  exit 1
fi

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== fim ==="
