#!/usr/bin/env bash
# Phase 2 slice: fault tolerance — a worker dies and the DAG still finishes.
#
# The acceptance criterion this checks: losing a worker does not corrupt the DAG.
# The worker is killed with no graceful leave, so the controller only notices when
# its heartbeat grace expires. Whatever the dead node held must end up either
# requeued to the surviving node or failed cleanly — never left outstanding.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051
# Size stays under the gRPC message ceiling; keep it there.
SIZE=${SIZE:-256}
KILL_AFTER=${KILL_AFTER:-10}
# Hold each claimed unit before computing it, so the kill lands while a unit is in
# flight. Without this the in-flight window is microseconds wide and the test only
# ever exercises the idle path. Longer than the 5s heartbeat interval on purpose:
# once a worker has claimed, it is holding the unit whenever the kill arrives.
WORK_DELAY_MS=${WORK_DELAY_MS:-12000}

run() {
  MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

echo "=== cleanup ==="
docker rm -f aic-controller aic-worker-b aic-worker-c >/dev/null 2>&1
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

echo "=== start controller + two workers ==="
# Grace must exceed the work hold below: a worker sends no heartbeat while it is
# holding a unit, so a short grace would reap a healthy worker mid-computation.
MSYS_NO_PATHCONV=1 docker run -d --name aic-controller --network "$NET" \
  -v aic-target:/t -e AI_CLUSTER_HEARTBEAT_GRACE_MS=20000 \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
for n in b c; do
  MSYS_NO_PATHCONV=1 docker run -d --name "aic-worker-$n" --network "$NET" \
    -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID="node-$n" \
    -e AI_CLUSTER_WORK_DELAY_MS="$WORK_DELAY_MS" \
    "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
done
sleep 8

echo "--- nodes before the failure ---"
run "$R/ai-cluster nodes"

echo "=== enqueue two units so both workers hold one, then kill a worker mid-flight ==="
# Two ready units and two workers: each worker claims one, so the node that dies is
# actually holding work. The diamond would not do this — it has a single ready head,
# so only one worker can ever be busy at a time.
run "$R/ai-cluster benchmark --size "$SIZE" --repetitions 2" &
CLI=$!
sleep "$KILL_AFTER"
echo "--- killing node-b: SIGKILL, no leave, no requeue on the way out ---"
docker kill aic-worker-b >/dev/null 2>&1
wait "$CLI"

echo "--- nodes after the failure ---"
run "$R/ai-cluster nodes"

echo "--- controller log ---"
docker logs aic-controller 2>&1 | tail -20
