#!/usr/bin/env bash
# Fatia da Fase G4: fila local por recurso (slots) no worker.
#
# Critério verificado: um worker com AI_CLUSTER_WORK_SLOTS=4 roda quatro unidades ao mesmo tempo
# e termina o lote em uma fração do tempo do mesmo worker com um slot só.
# AI_CLUSTER_WORK_DELAY_MS segura cada unidade 600 ms, então o tempo é dominado pela espera e
# não pelo compute: 8 unidades levam ~4,8 s com 1 slot e ~1,2 s com 4.
#
# Sequência:
#   A) 1 slot: o lote de 8 unidades leva pelo menos 4 s;
#   B) 4 slots: o mesmo lote leva menos da metade do tempo de A.
set -uo pipefail

NET=aic-net-g4s
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller-g4s:50051
ENVS="-e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1"

run() {
  MSYS_NO_PATHCONV=1 docker run $ENVS --rm --network "$NET" -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

teardown() {
  docker rm -f aic-controller-g4s aic-worker-g4s >/dev/null 2>&1
}

# Starts a fresh controller and one worker with the given slots, runs the batch and prints
# the wall time in milliseconds on the last line.
timed_batch() {
  local slots=$1
  teardown
  MSYS_NO_PATHCONV=1 docker run $ENVS -d --name aic-controller-g4s --network "$NET" -v aic-target:/t \
    "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
  MSYS_NO_PATHCONV=1 docker run $ENVS -d --name aic-worker-g4s --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-g4s \
    -e AI_CLUSTER_WORK_SLOTS="$slots" -e AI_CLUSTER_WORK_DELAY_MS=600 \
    "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
  sleep 8
  run "start=\$(date +%s%N); $R/ai-cluster benchmark --size 32 --repetitions 8 | grep concluidas; end=\$(date +%s%N); echo \$(( (end - start) / 1000000 ))"
}

docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

echo "=== A) 1 slot ==="
OUT1=$(timed_batch 1)
echo "$OUT1"
T1=$(echo "$OUT1" | tail -1)
echo "tempo=${T1}ms"
if [ "$T1" -ge 4000 ]; then
  echo "OK: com 1 slot o lote leva ${T1}ms (>= 4000)"
else
  echo "FALHA: esperado >= 4000ms com 1 slot, veio ${T1}ms"
  teardown
  exit 1
fi

echo "=== B) 4 slots ==="
OUT4=$(timed_batch 4)
echo "$OUT4"
T4=$(echo "$OUT4" | tail -1)
echo "tempo=${T4}ms"
if echo "$OUT4" | grep -q "concluidas=8 pendentes=0 falharam=0" && [ $((T4 * 2)) -lt "$T1" ]; then
  echo "OK: com 4 slots o lote leva ${T4}ms, menos da metade de ${T1}ms"
else
  echo "FALHA: esperado concluidas=8 e tempo < metade de ${T1}ms, veio ${T4}ms"
  teardown
  exit 1
fi

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== fim ==="
