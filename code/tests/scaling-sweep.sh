#!/usr/bin/env bash
# First experiment (§20): at what unit size does distributing beat the transfer cost?
#
# Runs the same workload at several matrix sizes across 1, 2 and 3 workers and
# records per-unit compute and transfer. Compute grows with the cube of the edge
# length; transfer grows with its square. The size where compute overtakes transfer
# is the crossover: below it, moving the work costs more than the work itself, so
# extra nodes do not pay for themselves.
#
# Sizes stay under the gRPC message ceiling. A size-S unit carries two SxS f64
# matrices, so S=512 is already ~4MB, at tonic's default decode limit.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051
NODES=${NODES:-"1 2 3"}
SIZES=${SIZES:-"32 64 128 256"}
REPS=${REPS:-6}

run() {
  MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" -v aic-target:/t \
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
  MSYS_NO_PATHCONV=1 docker run -d --name aic-controller --network "$NET" \
    -v aic-target:/t "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
  for n in $(seq 1 "$nodes"); do
    MSYS_NO_PATHCONV=1 docker run -d --name "aic-worker-$n" --network "$NET" \
      -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID="node-$n" \
      "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
  done
  sleep 8

  for size in $SIZES; do
    echo "=== nodes=$nodes size=$size reps=$REPS ==="
    start=$(date +%s)
    run "$R/ai-cluster benchmark --size "$size" --repetitions $REPS"
    end=$(date +%s)
    echo "wall_seconds=$((end - start)) nodes=$nodes size=$size"
  done
done

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== sweep complete ==="
