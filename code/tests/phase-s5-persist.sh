#!/usr/bin/env bash
# Fatia da Fase S (S5): a aprovação sobrevive a reinícios, e `revoke` retira a confiança.
#
# Critério verificado: sem AI_CLUSTER_AUTO_APPROVE, um nó aprovado continua aprovado quando o
# worker reinicia e quando o controller reinicia, desde que o worker guarde a credencial e o
# controller guarde o arquivo de nós. `revoke` devolve o nó a pendente e derruba o worker.
#
# Sequência:
#   A) worker novo fica pendente; `approve` o aprova.
#   B) o worker reinicia (sai com `leave_node`) e volta aprovado, sem novo `approve`.
#   C) o controller reinicia e o nó volta aprovado, sem novo `approve`.
#   D) `revoke`: o nó fica pendente e o worker encerra por credencial inválida.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051
JOIN=s5-token
OPERATOR=s5-operator

teardown() {
  docker rm -f aic-controller aic-worker aic-impostor >/dev/null 2>&1
}

fail() {
  echo "FALHA: $1"
  teardown
  exit 1
}

cli() {
  MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_OPERATOR_TOKEN=$OPERATOR \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$R/ai-cluster $*" 2>&1
}

# trust <nó>: aprovado | pendente | ausente
trust() {
  local row
  row=$(cli nodes | grep -E "^$1 ")
  if [ -z "$row" ]; then echo ausente; elif echo "$row" | grep -q aprovado; then echo aprovado; else echo pendente; fi
}

start_controller() {
  MSYS_NO_PATHCONV=1 docker run -d --name aic-controller --network "$NET" -v aic-target:/t -v aic-state:/state \
    -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=$JOIN -e AI_CLUSTER_OPERATOR_TOKEN=$OPERATOR \
    -e AI_CLUSTER_NODES=/state/nodes "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
}

teardown
docker volume rm aic-state aic-cred >/dev/null 2>&1
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

start_controller
sleep 2
MSYS_NO_PATHCONV=1 docker run -d --name aic-worker --network "$NET" -v aic-target:/t -v aic-cred:/cred \
  -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=$JOIN -e AI_CLUSTER_CONTROLLER=$CONTROLLER \
  -e AI_CLUSTER_NODE_ID=node-p -e AI_CLUSTER_NODE_CREDENTIAL_FILE=/cred/node-p \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 8

echo "=== A) nó novo pendente; approve o aprova ==="
[ "$(trust node-p)" = pendente ] || fail "node-p deveria nascer pendente"
cli approve node-p --trust-listed | grep -q aprovado || fail "approve deveria confirmar"
[ "$(trust node-p)" = aprovado ] || fail "node-p deveria estar aprovado"
echo "OK: node-p aprovado"

echo "=== B) o worker reinicia e continua aprovado ==="
docker restart aic-worker >/dev/null
sleep 10
[ "$(trust node-p)" = aprovado ] || fail "node-p deveria continuar aprovado depois de reiniciar o worker"
echo "OK: node-p continua aprovado depois do reinício do worker"

echo "=== C) o controller reinicia e o nó continua aprovado ==="
docker restart aic-controller >/dev/null
sleep 5
# O worker não sobrevive a um controller fora do ar (o poll de trabalho propaga o erro); um
# orquestrador o reergueria. `docker start` faz esse papel e não faz nada se ele ainda roda.
docker start aic-worker >/dev/null
sleep 12
[ "$(trust node-p)" = aprovado ] || fail "node-p deveria continuar aprovado depois de reiniciar o controller"
echo "OK: node-p continua aprovado depois do reinício do controller"

echo "=== C2) quem só tem o token de join não toma o id de um nó aprovado ==="
MSYS_NO_PATHCONV=1 docker run -d --name aic-impostor --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=$JOIN -e AI_CLUSTER_CONTROLLER=$CONTROLLER \
  -e AI_CLUSTER_NODE_ID=node-p "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 6
if docker logs aic-impostor 2>&1 | grep -q "ai-cluster revoke"; then
  echo "OK: o impostor foi recusado e o aviso manda revogar"
else
  docker logs aic-impostor 2>&1 | tail -3
  fail "o impostor deveria ser recusado"
fi
docker rm -f aic-impostor >/dev/null 2>&1
[ "$(trust node-p)" = aprovado ] || fail "node-p deveria continuar aprovado depois da tentativa do impostor"
echo "OK: node-p continua aprovado"

echo "=== D) revoke devolve o nó a pendente e derruba o worker ==="
OUT=$(cli revoke node-p)
echo "$OUT"
echo "$OUT" | grep -q revogado || fail "revoke deveria confirmar"
[ "$(trust node-p)" = pendente ] || fail "node-p deveria ficar pendente depois do revoke"
sleep 8
if [ "$(docker inspect -f '{{.State.Running}}' aic-worker)" = false ] \
  && docker logs aic-worker 2>&1 | grep -qi "unauthenticated"; then
  echo "OK: o worker revogado encerrou com Unauthenticated"
else
  docker logs aic-worker 2>&1 | tail -3
  fail "o worker revogado deveria encerrar com Unauthenticated"
fi
cli revoke ghost | grep -q "não encontrado" || fail "revoke de nó desconhecido deveria responder 'não encontrado'"

teardown
echo "=== S5 persistência e revogação OK ==="
