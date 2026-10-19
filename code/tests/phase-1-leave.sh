#!/usr/bin/env bash
# Phase 1 exit criterion: nodes join and leave safely.
#
# Flow: controller + two workers -> enqueue work -> stop one worker with SIGTERM
# (planned leave) -> the survivor must finish every unit, with nothing stranded.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051
LOG=/tmp/aic-leave-bench.log

run() { MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" -v aic-target:/t \
        -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"; }

echo "=== cleanup ==="
docker rm -f aic-controller aic-worker1 aic-worker2 >/dev/null 2>&1 || true
docker network rm "$NET" >/dev/null 2>&1 || true
docker network create "$NET" >/dev/null

echo "=== controller ==="
MSYS_NO_PATHCONV=1 docker run -d --name aic-controller --network "$NET" \
  -v aic-target:/t "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null

echo "=== worker1 (node-b) ==="
MSYS_NO_PATHCONV=1 docker run -d --name aic-worker1 --network "$NET" \
  -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-b \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null

sleep 6
echo "=== nodes before leave ==="
run "$R/ai-cluster nodes"

echo "=== enqueue 6 units (polls until the queue drains) ==="
run "$R/ai-cluster benchmark --size 128 --repetitions 6" >"$LOG" 2>&1 &
BENCH=$!
sleep 4

echo "=== worker2 (node-c) joins mid-run ==="
MSYS_NO_PATHCONV=1 docker run -d --name aic-worker2 --network "$NET" \
  -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-c \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 8

echo "=== graceful leave: docker stop aic-worker1 ==="
docker stop aic-worker1
sleep 2

echo "=== nodes after leave ==="
run "$R/ai-cluster nodes"

echo "=== waiting for the benchmark to drain ==="
wait $BENCH
echo "--- benchmark output ---"
cat "$LOG"

echo "--- controller log ---"
docker logs aic-controller 2>&1 | tail -15

echo "=== worker1 log (graceful leave) ==="
docker logs aic-worker1 2>&1 | tail -8
