#!/usr/bin/env bash
# Fatia da Fase S (S2): aprovação de nó novo.
#
# Critério verificado: um nó que acabou de se registrar fica pendente e não recebe trabalho
# até alguém rodar `ai-cluster approve`; a aprovação sobrevive a um re-registro. O token de
# join (S1) prova quem pode falar com o controller; a aprovação decide quem pode trabalhar.
#
# Sequência:
#   A) nó novo: aparece como `pendente`, e uma unidade enfileirada não é entregue a ele.
#   B) `approve`: o nó vira `aprovado` e a unidade que esperava roda nele.
#   C) `approve` de um nó desconhecido responde "não encontrado".
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051
TOKEN=s2-secret
SIZE=64

teardown() {
  docker rm -f aic-controller aic-worker aic-bench >/dev/null 2>&1
}

OPERATOR=s2-operator

cli() {
  MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_OPERATOR_TOKEN=${CLI_TOKEN:-$OPERATOR} -e AI_CLUSTER_CONTROLLER=$CONTROLLER \
    "$IMG" sh -c "$R/ai-cluster $*" 2>&1
}

fail() {

  echo "FALHA: $1"
  teardown
  exit 1
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

# Sem AI_CLUSTER_AUTO_APPROVE: o padrão é exigir aprovação.
MSYS_NO_PATHCONV=1 docker run -d --name aic-controller --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_JOIN_TOKEN=$TOKEN -e AI_CLUSTER_OPERATOR_TOKEN=$OPERATOR \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
MSYS_NO_PATHCONV=1 docker run -d --name aic-worker --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_JOIN_TOKEN=$TOKEN -e AI_CLUSTER_CONTROLLER=$CONTROLLER \
  -e AI_CLUSTER_NODE_ID=node-new "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 8

echo "=== A) nó novo fica pendente e não recebe trabalho ==="
NODES=$(cli nodes)
echo "$NODES"
if echo "$NODES" | grep -E "^node-new " | grep -q pendente; then
  echo "OK: node-new está pendente"
else
  fail "node-new deveria estar pendente"
fi
MSYS_NO_PATHCONV=1 docker run -d --name aic-bench --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_OPERATOR_TOKEN=$OPERATOR -e AI_CLUSTER_CONTROLLER=$CONTROLLER \
  "$IMG" sh -c "$R/ai-cluster benchmark --size $SIZE --repetitions 1" >/dev/null
sleep 10
if docker logs aic-bench 2>&1 | grep -q '^w-'; then
  docker logs aic-bench 2>&1 | tail -3
  fail "uma unidade foi entregue a um nó pendente"
fi
echo "OK: a unidade esperou, o nó pendente não recebeu trabalho"

echo "=== A2) o token de join não aprova nó (S3a) ==="
DENIED=$(CLI_TOKEN=$TOKEN cli approve node-new)
echo "$DENIED"
if echo "$DENIED" | grep -qi "operador"; then
  echo "OK: approve com o token de join foi recusado"
else
  fail "approve com o token de join deveria ser recusado"
fi
if cli nodes | grep -E "^node-new " | grep -q pendente; then
  echo "OK: node-new continua pendente"
else
  fail "node-new não deveria ter sido aprovado pelo token de join"
fi

echo "=== B) approve libera o nó e a unidade que esperava ==="
OUT=$(cli approve node-new)
echo "$OUT"
echo "$OUT" | grep -q "aprovado" || fail "approve deveria confirmar a aprovação"
sleep 8
if docker logs aic-bench 2>&1 | grep '^w-' | grep -q node-new; then
  echo "OK: a unidade rodou em node-new depois da aprovação"
else
  docker logs aic-bench 2>&1 | tail -5
  fail "a unidade deveria ter rodado em node-new"
fi
if cli nodes | grep -E "^node-new " | grep -q aprovado; then
  echo "OK: node-new consta como aprovado"
else
  fail "node-new deveria constar como aprovado"
fi

echo "=== C) nó desconhecido ==="
if cli approve ghost | grep -q "não encontrado"; then
  echo "OK: approve de um nó desconhecido responde 'não encontrado'"
else
  fail "approve ghost deveria responder 'não encontrado'"
fi

teardown
echo "=== S2 aprovação OK ==="
