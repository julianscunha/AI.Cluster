#!/usr/bin/env bash
# Phase 2 slice: dependency graph dispatch.
#
# Flow: controller + two workers -> enqueue the diamond A -> (B, C) -> D.
# The point is ordering: B and C must not be dispatched until A completes, and D
# must not be dispatched until both B and C complete.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051

run() {
  MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

echo "=== cleanup ==="
docker rm -f aic-controller aic-worker1 aic-worker2 >/dev/null 2>&1
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

echo "=== start controller + two workers ==="
MSYS_NO_PATHCONV=1 docker run -d --name aic-controller --network "$NET" \
  -v aic-target:/t "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
for pair in "aic-worker1:node-b" "aic-worker2:node-c"; do
  name="${pair%%:*}"
  node="${pair##*:}"
  MSYS_NO_PATHCONV=1 docker run -d --name "$name" --network "$NET" \
    -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID="$node" \
    "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
done
sleep 8

echo "--- nodes ---"
run "$R/ai-cluster nodes"

echo "=== enqueue the diamond and wait for it to drain ==="
run "$R/ai-cluster graph --size 128"

echo "--- controller log ---"
docker logs aic-controller 2>&1 | tail -15
