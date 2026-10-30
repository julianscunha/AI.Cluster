#!/usr/bin/env bash
# Fatia da Fase H (H3): correlacionar a requisição do cliente com as unidades que ela gerou.
#
# Critério verificado: dado o id de uma requisição, o cliente vê todas as unidades que ela
# produziu, em que nó cada uma rodou e quanto custou de compute, transferência e ida e volta
# total. É a resposta a "por que essa requisição demorou?".
#
# Sequência:
#   A) enfileirar 2 unidades e capturar o request id que o controlador cunhou.
#   B) o trace mostra as 2 unidades concluídas, com compute somado > 0.
#   C) um id desconhecido devolve trace vazio, sem erro.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051
SIZE=64

run() {
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

teardown() {
  docker rm -f aic-controller aic-worker-t >/dev/null 2>&1
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-controller --network "$NET" -v aic-target:/t \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-worker-t --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-t \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 8

echo "=== A) enfileirar 2 unidades ==="
OUT=$(run "$R/aic cluster benchmark --size $SIZE --repetitions 2")
echo "$OUT"
REQ=$(echo "$OUT" | sed -n 's/.*requisicao=\([^ ]*\).*/\1/p' | head -1)
if [ -z "$REQ" ]; then
  echo "FALHA: o benchmark não imprimiu requisicao=, então não há o que rastrear"
  teardown
  exit 1
fi
echo "requisicao capturada: $REQ"

echo "=== B) trace da requisição ==="
TRACE=$(run "$R/aic cluster request trace $REQ")
echo "$TRACE"
COMPLETED=$(echo "$TRACE" | grep -c "completed")
if [ "$COMPLETED" = "2" ]; then
  echo "OK: as 2 unidades da requisição aparecem como concluídas"
else
  echo "FALHA: esperado 2 linhas completed no trace, veio $COMPLETED"
  teardown
  exit 1
fi
COMPUTE=$(echo "$TRACE" | grep -o "compute_somado_us=[0-9]*" | cut -d= -f2)
if [ "$COMPUTE" -gt 0 ] 2>/dev/null; then
  echo "OK: o compute somado do trace é maior que zero ($COMPUTE us)"
else
  echo "FALHA: compute_somado_us deveria ser > 0, veio '$COMPUTE'"
  teardown
  exit 1
fi

echo "=== C) id desconhecido ==="
UNKNOWN=$(run "$R/aic cluster request trace nao-existe")
echo "$UNKNOWN"
if echo "$UNKNOWN" | grep -q "UNIDADES 0"; then
  echo "OK: id desconhecido devolve trace vazio, sem erro"
else
  echo "FALHA: id desconhecido deveria devolver UNIDADES 0"
  teardown
  exit 1
fi

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== fim ==="
