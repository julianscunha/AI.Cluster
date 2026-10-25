#!/usr/bin/env bash
# Fatia da Fase I (F06): a linha do tempo de eventos sobrevive a um restart do controller.
#
# Critério verificado: um worker entra, o controller é derrubado e sobe de novo com o mesmo
# AI_CLUSTER_EVENTS, e `ai-cluster events` ainda mostra o evento de entrada do nó.
set -uo pipefail

NET=aic-net-i6
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller-i6:50051
ENVS="-e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=e2e-token -e AI_CLUSTER_OPERATOR_TOKEN=e2e-operator -e AI_CLUSTER_AUTO_APPROVE=1"

run() { MSYS_NO_PATHCONV=1 docker run $ENVS --rm --network "$NET" -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"; }
teardown() { docker rm -f aic-controller-i6 aic-worker-i6 >/dev/null 2>&1; docker volume rm aic-events-i6 >/dev/null 2>&1; }
start_controller() {
  MSYS_NO_PATHCONV=1 docker run $ENVS -d --name aic-controller-i6 --network "$NET" -v aic-target:/t -v aic-events-i6:/ev \
    -e AI_CLUSTER_EVENTS=/ev/events.log "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null
docker volume create aic-events-i6 >/dev/null
start_controller
MSYS_NO_PATHCONV=1 docker run $ENVS -d --name aic-worker-i6 --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-i6 "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 8

echo "=== A) o evento de entrada existe antes do restart ==="
BEFORE=$(run "$R/ai-cluster events --limit 50")
echo "$BEFORE"
echo "$BEFORE" | grep -q node-i6 || { echo "FALHA: sem evento do nó antes do restart"; teardown; exit 1; }
echo "OK"

echo "=== B) depois de derrubar e subir o controller o evento continua ==="
docker rm -f aic-controller-i6 >/dev/null 2>&1
start_controller
sleep 3
AFTER=$(run "$R/ai-cluster events --limit 50")
echo "$AFTER"
if echo "$AFTER" | grep -q node-i6; then
  echo "OK: a linha do tempo sobreviveu ao restart"
else
  echo "FALHA: o evento do nó sumiu no restart"
  teardown
  exit 1
fi

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== fim ==="
