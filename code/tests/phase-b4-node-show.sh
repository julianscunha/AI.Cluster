#!/usr/bin/env bash
# Fatia da Fase B (B4): `ai-cluster node show <id>`.
#
# Critério verificado: um nó pode ser inspecionado sozinho, com os campos que a tabela de
# `nodes` não mostra — compute medido, classes de armazenamento, pressão de memória e a
# reserva do nó. Sem isso, ver esses campos exige ler o código do worker.
#
# Sequência:
#   A) detalhe de um nó vivo: estado, capability e saúde.
#   B) a reserva do nó aparece no detalhe (G6 e B4 juntos).
#   C) id desconhecido é informativo, não erro.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051

run() {
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

teardown() {
  docker rm -f aic-controller aic-worker-b4 >/dev/null 2>&1
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-controller --network "$NET" -v aic-target:/t \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-worker-b4 --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-b4 \
  -e AI_CLUSTER_STORAGE_CLASSES=nvme,ssd \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 8

echo "=== A) detalhe de um nó vivo ==="
DETAIL=$(run "$R/ai-cluster node show node-b4")
echo "$DETAIL"
for field in "estado=READY" "armazenamento=nvme,ssd" "threads=" "score=" "compute_score="; do
  if echo "$DETAIL" | grep -q "$field"; then
    echo "OK: o detalhe traz '$field'"
  else
    echo "FALHA: '$field' não apareceu no detalhe"
    teardown
    exit 1
  fi
done

echo "=== B) a reserva do nó aparece no detalhe ==="
run "$R/ai-cluster reserve node-b4 req-b4" >/dev/null
HELD=$(run "$R/ai-cluster node show node-b4")
if echo "$HELD" | grep -q "requisicao=req-b4"; then
  echo "OK: o detalhe mostra a reserva de req-b4"
else
  echo "FALHA: esperado requisicao=req-b4, veio: $HELD"
  teardown
  exit 1
fi

echo "=== C) id desconhecido ==="
MISSING=$(run "$R/ai-cluster node show node-nao-existe")
if echo "$MISSING" | grep -q "não encontrado"; then
  echo "OK: id desconhecido é informativo ('$MISSING')"
else
  echo "FALHA: esperado 'não encontrado', veio '$MISSING'"
  teardown
  exit 1
fi

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== fim ==="
