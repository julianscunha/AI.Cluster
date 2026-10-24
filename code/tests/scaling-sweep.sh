#!/usr/bin/env bash
# Primeiro experimento (§20): a partir de que tamanho de unidade distribuir compensa o custo de transferência?
#
# Roda a mesma carga em vários tamanhos de matriz com 1, 2 e 3 workers e
# registra compute e transferência por unidade. O compute cresce com o cubo da aresta
# e a transferência com o quadrado. O tamanho em que o compute supera a transferência
# é o cruzamento: abaixo dele, mover o trabalho custa mais que o próprio trabalho, então
# nós extras não se pagam.
#
# Os tamanhos ficam abaixo do teto de mensagem do gRPC. Uma unidade de tamanho S carrega duas matrizes f64 SxS,
# então S=512 já é ~4MB, no limite de decode padrão do tonic.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051
NODES=${NODES:-"1 2 3"}
SIZES=${SIZES:-"32 64 128 256"}
REPS=${REPS:-6}

run() {
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

teardown() {
  for n in 1 2 3 4; do docker rm -f "aic-worker-$n" >/dev/null 2>&1; done
  docker rm -f aic-controller >/dev/null 2>&1
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

for nodes in $NODES; do
  teardown
  echo "=== cluster: controller + $nodes worker(s) ==="
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-controller --network "$NET" \
    -v aic-target:/t "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
  for n in $(seq 1 "$nodes"); do
    MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name "aic-worker-$n" --network "$NET" \
      -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID="node-$n" \
      "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
  done
  sleep 8

  for size in $SIZES; do
    echo "=== nós=$nodes tamanho=$size reps=$REPS ==="
    start=$(date +%s)
    run "$R/ai-cluster benchmark --size "$size" --repetitions $REPS"
    end=$(date +%s)
    echo "wall_seconds=$((end - start)) nodes=$nodes size=$size"
  done
done

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== varredura completa ==="
