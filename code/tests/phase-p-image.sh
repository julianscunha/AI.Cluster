#!/usr/bin/env bash
# Fatia da Fase P (F15, J1/J2): a imagem mínima do worker sobe e entra no cluster.
# Constrói `os/Dockerfile.worker` do zero (compila o worker; leva alguns minutos na primeira vez).
set -uo pipefail
NET=aic-net-pi
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller-pi:50051
teardown() { docker rm -f aic-controller-pi aic-worker-pi >/dev/null 2>&1; }
fail() { echo "FALHA: $1"; teardown; exit 1; }

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

echo "=== A) construir a imagem ==="
docker build -q -f os/Dockerfile.worker -t ai-cluster-worker:dev . >/dev/null || fail "o build da imagem falhou"
SIZE=$(docker image inspect ai-cluster-worker:dev --format '{{.Size}}')
echo "imagem de $((SIZE / 1000000)) MB"
USER_OF=$(docker image inspect ai-cluster-worker:dev --format '{{.Config.User}}')
[ "$USER_OF" = ai-cluster ] || fail "a imagem deveria rodar como ai-cluster, roda como '$USER_OF'"
echo "OK: construída, usuário sem privilégios"

echo "=== B) a imagem vira um nó READY ==="
MSYS_NO_PATHCONV=1 docker run -d --name aic-controller-pi --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=pi-join -e AI_CLUSTER_OPERATOR_TOKEN=pi-operator \
  -e AI_CLUSTER_AUTO_APPROVE=1 "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
docker run -d --name aic-worker-pi --network "$NET" -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=pi-join \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-image ai-cluster-worker:dev >/dev/null
STATE=""
for _ in $(seq 1 30); do
  STATE=$(MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" -v aic-target:/t -e AI_CLUSTER_INSECURE=1 \
    -e AI_CLUSTER_OPERATOR_TOKEN=pi-operator -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$R/aic cluster nodes" 2>/dev/null \
    | awk '$1 == "node-image" { print $2 }')
  [ "$STATE" = READY ] && break
  sleep 2
done
[ "$STATE" = READY ] || fail "node-image deveria estar READY, está '$STATE'"
echo "OK: node-image READY"

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== fim ==="
