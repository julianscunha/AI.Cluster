#!/usr/bin/env bash
# Fatia da Fase 2: dispatch do grafo de dependência.
#
# Fluxo: controller + dois workers -> enfileirar o diamante A -> (B, C) -> D.
# O ponto é a ordenação: B e C não devem ser despachados até A completar, e D
# não deve ser despachado até B e C completarem.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051

run() {
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_AUTO_APPROVE=1 --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

echo "=== limpeza ==="
docker rm -f aic-controller aic-worker1 aic-worker2 >/dev/null 2>&1
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

echo "=== iniciar controller + dois workers ==="
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-controller --network "$NET" \
  -v aic-target:/t "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
for pair in "aic-worker1:node-b" "aic-worker2:node-c"; do
  name="${pair%%:*}"
  node="${pair##*:}"
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name "$name" --network "$NET" \
    -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID="$node" \
    "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
done
sleep 8

echo "--- nós ---"
run "$R/ai-cluster nodes"

echo "=== enfileirar o diamante e aguardar drenar ==="
run "$R/ai-cluster graph --size 128"

echo "--- log do controller ---"
docker logs aic-controller 2>&1 | tail -15
