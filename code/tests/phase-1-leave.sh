#!/usr/bin/env bash
# Critério de saída da Fase 1: nós entram e saem com segurança.
#
# Fluxo: controller + dois workers -> enfileirar trabalho -> parar um worker com SIGTERM
# (saída planejada) -> o sobrevivente deve terminar cada unidade, sem nada encalhado.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051
LOG=/tmp/aic-leave-bench.log

run() { MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_AUTO_APPROVE=1 --rm --network "$NET" -v aic-target:/t \
        -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"; }

echo "=== limpeza ==="
docker rm -f aic-controller aic-worker1 aic-worker2 >/dev/null 2>&1 || true
docker network rm "$NET" >/dev/null 2>&1 || true
docker network create "$NET" >/dev/null

echo "=== controller ==="
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-controller --network "$NET" \
  -v aic-target:/t "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null

echo "=== worker1 (node-b) ==="
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-worker1 --network "$NET" \
  -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-b \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null

sleep 6
echo "=== nós antes da saída ==="
run "$R/ai-cluster nodes"

echo "=== enfileirar 6 unidades (faz poll até a fila drenar) ==="
run "$R/ai-cluster benchmark --size 128 --repetitions 6" >"$LOG" 2>&1 &
BENCH=$!
sleep 4

echo "=== worker2 (node-c) entra no meio da execução ==="
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-worker2 --network "$NET" \
  -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-c \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 8

echo "=== saída graciosa: docker stop aic-worker1 ==="
docker stop aic-worker1
sleep 2

echo "=== nós após a saída ==="
run "$R/ai-cluster nodes"

echo "=== aguardando o benchmark drenar ==="
wait $BENCH
echo "--- saída do benchmark ---"
cat "$LOG"

echo "--- log do controller ---"
docker logs aic-controller 2>&1 | tail -15

echo "=== log do worker1 (saída graciosa) ==="
docker logs aic-worker1 2>&1 | tail -8
