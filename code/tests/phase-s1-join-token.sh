#!/usr/bin/env bash
# Fatia da Fase S (S1): token de join.
#
# Critério verificado: o controller só aceita quem apresenta o token de join, em todas as
# chamadas. Sem isso qualquer cliente da rede registrava um nó ou injetava estado pela fila.
#
# Sequência:
#   A) controller sem token: recusa subir (fail-closed), não sobe aberto.
#   B) worker com token errado e worker sem token: não entram, o log mostra Unauthenticated.
#   C) worker com o token certo: entra e fica READY.
#   D) a CLI sem token também é recusada; com o token lista o nó.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051
TOKEN=s1-secret

teardown() {
  docker rm -f aic-controller aic-w-bad aic-w-none aic-w-ok >/dev/null 2>&1
}

# cli <token|-> <args...> — roda a CLI; "-" é sem token.
cli() {
  local token=$1
  shift
  local env=()
  [ "$token" != "-" ] && env=(-e "AI_CLUSTER_OPERATOR_TOKEN=$token")
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 --rm --network "$NET" -v aic-target:/t "${env[@]}" \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$R/ai-cluster $*" 2>&1
}

# worker <nome> <nó> <token|->
worker() {
  local env=()
  [ "$3" != "-" ] && env=(-e "AI_CLUSTER_JOIN_TOKEN=$3")
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -d --name "$1" --network "$NET" -v aic-target:/t "${env[@]}" \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID="$2" \
    "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
}

fail() {
  echo "FALHA: $1"
  teardown
  exit 1
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

echo "=== A) controller sem token recusa subir ==="
OUT=$(MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 --rm --network "$NET" -v aic-target:/t "$IMG" \
  sh -c "$R/ai-cluster-controller" 2>&1)
if echo "$OUT" | grep -q "AI_CLUSTER_JOIN_TOKEN"; then
  echo "OK: o controller sem token não subiu"
else
  fail "o controller deveria recusar subir sem token, saiu: $OUT"
fi

MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -d --name aic-controller --network "$NET" -v aic-target:/t \
  -e "AI_CLUSTER_JOIN_TOKEN=$TOKEN" -e AI_CLUSTER_OPERATOR_TOKEN=s1-operator   "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null

echo "=== B) token errado e token ausente não entram ==="
worker aic-w-bad node-bad wrong-token
worker aic-w-none node-none -
sleep 8
NODES=$(cli "$TOKEN" nodes)
if echo "$NODES" | grep -qE "node-bad|node-none"; then
  echo "$NODES"
  fail "um nó sem o token certo apareceu em nodes"
fi
for w in aic-w-bad aic-w-none; do
  if docker logs "$w" 2>&1 | grep -qi "unauthenticated"; then
    echo "OK: $w foi recusado com Unauthenticated"
  else
    docker logs "$w" 2>&1 | tail -3
    fail "$w deveria ter recebido Unauthenticated"
  fi
done

echo "=== C) token certo entra ==="
worker aic-w-ok node-ok "$TOKEN"
sleep 8
NODES=$(cli "$TOKEN" nodes)
echo "$NODES"
if echo "$NODES" | grep -E "^node-ok " | grep -q READY; then
  echo "OK: node-ok entrou e está READY"
else
  fail "node-ok deveria estar READY"
fi

echo "=== D) a CLI sem token é recusada ==="
OUT=$(cli - nodes)
if echo "$OUT" | grep -qi "unauthenticated" && ! echo "$OUT" | grep -q "node-ok"; then
  echo "OK: a CLI sem token foi recusada e não viu nós"
else
  echo "$OUT"
  fail "a CLI sem token deveria ser recusada"
fi

teardown
echo "=== S1 token de join OK ==="
