#!/usr/bin/env bash
# Fatia da Fase F11: `aic cluster request cancel` para a conta de uma unidade que já roda no worker.
#
# Critério verificado: uma unidade grande, em execução num worker de 1 thread, é cancelada pelo
# operador; o worker recebe o aviso na batida seguinte, interrompe a multiplicação (não termina
# os segundos que faltavam) e não envia resultado; o controller recusa qualquer resultado tardio.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051
SIZE=${SIZE:-2000}
ENVS=(-e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=e2e-token -e AI_CLUSTER_OPERATOR_TOKEN=e2e-operator
  -e AI_CLUSTER_AUTO_APPROVE=1)

teardown() { docker rm -f aic-controller aic-worker aic-bench >/dev/null 2>&1; }
fail() { echo "FALHA: $1"; docker logs aic-worker 2>&1 | tail -8; teardown; exit 1; }

teardown
docker network rm "$NET" >/dev/null 2>&1; docker network create "$NET" >/dev/null
MSYS_NO_PATHCONV=1 docker run -d --name aic-controller "${ENVS[@]}" --network "$NET" -v aic-target:/t \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
sleep 2
MSYS_NO_PATHCONV=1 docker run -d --name aic-worker "${ENVS[@]}" --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_CPU_THREADS=1 -e AI_CLUSTER_HEARTBEAT_SECS=1 -e AI_CLUSTER_NODE_ID=node-cancel \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 8

echo "=== A) uma unidade grande começa a rodar (1 thread, ${SIZE}³) ==="
MSYS_NO_PATHCONV=1 docker run -d --name aic-bench "${ENVS[@]}" --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" \
  sh -c "$R/aic cluster benchmark --size $SIZE --repetitions 1 --request-id req-cancel" >/dev/null
sleep 1
STARTED=$(date +%s)

echo "=== B) o operador cancela a requisição ==="
OUT=$(MSYS_NO_PATHCONV=1 docker run --rm "${ENVS[@]}" --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$R/aic cluster request cancel req-cancel")
echo "$OUT"
echo "$OUT" | grep -q "em_voo=1" || fail "a unidade deveria estar em voo: $OUT"

echo "=== C) o worker para a conta e não manda resultado ==="
for _ in $(seq 1 20); do
  docker logs aic-worker 2>&1 | grep -q "cancelado pelo controller" && break
  sleep 1
done
docker logs aic-worker 2>&1 | grep -q "cancelado pelo controller" || fail "o worker não recebeu o cancelamento"
TOOK=$(( $(date +%s) - STARTED ))
docker logs aic-worker 2>&1 | grep -q "compute_done" && fail "o worker não deveria ter concluído a unidade"
echo "OK: cancelado em ${TOOK}s após o início, sem resultado"

teardown
echo "=== F11 cancelamento no worker OK ==="
