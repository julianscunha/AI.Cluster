#!/usr/bin/env bash
# Fatia da Fase I (I1): o diretório de estado do controller — snapshot primário e cópia.
#
# Critério verificado: o estado do cluster sobrevive a uma interrupção abrupta do controller,
# e perder o arquivo primário não perde a fila — a cópia ao lado é carregada no lugar.
#
# Sequência:
#   A) enfileira um grafo sem worker; os dois arquivos (primário e cópia) são escritos.
#   B) `docker kill` no controller, sem shutdown limpo — a fila não pode sumir com ele.
#   C) o primário é zerado; o controller tem de carregar a cópia, não "iniciar vazio".
#   D) um worker entra e drena a fila: o trabalho sobreviveu de ponta a ponta.
#
# A posse em voo — o dono da unidade atravessando o restart — não é observável aqui sem uma
# corrida contra o relógio, e é provada por teste unitário em
# `an_in_flight_unit_keeps_its_owner_across_a_restart` e
# `an_unknown_owner_is_listed_until_its_node_registers` (code/crates/controller/src/work.rs).
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051
STATE_DIR=/t/state-i1
STATE=$STATE_DIR/state.bin
REPLICA=$STATE.replica

run() {
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

# Runs a command against the shared volume without joining the cluster network.
in_vol() {
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 --rm -v aic-target:/t "$IMG" sh -c "$1"
}

teardown() {
  docker rm -f aic-controller aic-worker-i1 >/dev/null 2>&1
}

field() {
  grep -o "$1=[0-9]*" | head -1 | cut -d= -f2
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null
# A previous run leaves its snapshot behind; start from nothing so the counts mean something.
# The directory itself has to exist: the controller writes into it, it does not create it.
in_vol "rm -rf $STATE_DIR && mkdir -p $STATE_DIR"

echo "=== A) enfileirar um grafo sem worker ==="
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-controller --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_STATE=$STATE "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
sleep 4
# O cliente do grafo espera a fila drenar, e não há worker — é cortado assim que as unidades
# entram. O que importa é o enfileiramento, que é o que grava o snapshot.
timeout 8 env MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 --rm --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" \
  sh -c "$R/ai-cluster graph --size 128" >/dev/null 2>&1 || true

if in_vol "test -s $STATE && test -s $REPLICA"; then
  echo "OK: primário e cópia escritos"
else
  echo "FALHA: o primário ou a cópia está ausente ou vazio"
  teardown
  exit 1
fi

echo "=== B) interrupção abrupta do controller ==="
docker kill aic-controller >/dev/null
docker rm -f aic-controller >/dev/null 2>&1
echo "OK: controller morto sem shutdown limpo"

echo "=== C) zerar o primário; a cópia tem de carregar ==="
in_vol ": > $STATE"
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-controller --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_STATE=$STATE "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
sleep 4
LOADED=$(docker logs aic-controller 2>&1 | grep "estado carregado" | head -1)
echo "$LOADED"
if echo "$LOADED" | grep -q "state.bin.replica"; then
  echo "OK: carregou da cópia depois de o primário ser zerado"
else
  echo "FALHA: esperado carregar de state.bin.replica, veio '$LOADED'"
  teardown
  exit 1
fi
PENDING=$(echo "$LOADED" | grep -oE '[0-9]+ pendente' | head -1 | cut -d' ' -f1)
if [ "${PENDING:-0}" -ge 1 ] 2>/dev/null; then
  echo "OK: a fila voltou com $PENDING unidade(s)"
else
  echo "FALHA: a cópia devia trazer unidades de volta, veio '$LOADED'"
  teardown
  exit 1
fi

echo "=== D) um worker drena a fila restaurada ==="
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-worker-i1 --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-i1 \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
PEND=""; DONE=""
for _ in $(seq 1 40); do
  REPORT=$(run "$R/ai-cluster metrics" 2>/dev/null)
  PEND=$(echo "$REPORT" | field units_pending)
  DONE=$(echo "$REPORT" | field units_completed)
  [ "$PEND" = "0" ] && [ "$DONE" = "4" ] && break
  sleep 2
done
echo "$REPORT"
if [ "$PEND" = "0" ] && [ "$DONE" = "4" ]; then
  echo "OK: 4 unidades concluídas a partir da fila que sobreviveu ao kill"
else
  echo "FALHA: esperado units_pending=0 units_completed=4, veio pending=$PEND completed=$DONE"
  teardown
  exit 1
fi

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== fim ==="