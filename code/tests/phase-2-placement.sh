#!/usr/bin/env bash
# Phase 2 slice: placement — a unit's requirement decides which node runs it.
#
# The acceptance criterion this checks: the scheduler picks placement better than
# first-come-first-served. A unit that declares min_cpu_threads is refused to a
# node reporting fewer, so it waits for a capable node instead of being handed to
# the first one that polls. Without the filter the weak node takes it and the run
# finishes with the unit on node-weak.
#
# Sequence: bring up a weak node alone, enqueue a unit that needs more threads than
# it has, show the unit stays unfinished, then add a strong node and show the unit
# goes to it. The weak node's capability is forced down with AI_CLUSTER_CPU_THREADS
# so a single host can stand in for two machines of different sizes.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051
SIZE=${SIZE:-256}
WEAK_THREADS=${WEAK_THREADS:-2}
STRONG_THREADS=${STRONG_THREADS:-8}
MIN_THREADS=${MIN_THREADS:-4}

run() {
  MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

echo "=== cleanup ==="
docker rm -f aic-controller aic-worker-weak aic-worker-strong >/dev/null 2>&1
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

echo "=== start controller + the weak node only ==="
MSYS_NO_PATHCONV=1 docker run -d --name aic-controller --network "$NET" \
  -v aic-target:/t "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
MSYS_NO_PATHCONV=1 docker run -d --name aic-worker-weak --network "$NET" \
  -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-weak \
  -e AI_CLUSTER_CPU_THREADS="$WEAK_THREADS" \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 8

echo "--- nodes: the weak node reports $WEAK_THREADS threads ---"
run "$R/ai-cluster nodes"

echo "=== enqueue a unit needing $MIN_THREADS threads; only the weak node is present ==="
# The client polls until the queue drains, which cannot happen while the only node
# present is too weak to take the unit. So it runs in the background and is expected
# to still be polling when the strong node is added below.
MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" \
  sh -c "$R/ai-cluster benchmark --size $SIZE --repetitions 1 --min-threads $MIN_THREADS" &
CLI=$!
sleep 12

echo "--- controller log during the weak-only window ---"
if docker logs aic-controller 2>&1 | grep -q "finished on"; then
  echo "FAIL: a unit finished while only the weak node was present"
else
  echo "OK: nothing finished; the weak node did not take the heavy unit"
fi

echo "=== start the strong node; the waiting unit should go to it ==="
MSYS_NO_PATHCONV=1 docker run -d --name aic-worker-strong --network "$NET" \
  -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-strong \
  -e AI_CLUSTER_CPU_THREADS="$STRONG_THREADS" \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
wait "$CLI"

echo "--- controller log ---"
docker logs aic-controller 2>&1 | tail -12
