#!/usr/bin/env bash
# Fatia da Fase I (I4): recuperação — uma unidade em voo atravessa o restart do controller e a
# perda do dono, e termina em outro nó.
#
# Critério verificado: interrupção abrupta do controller *com* unidade em voo não perde
# trabalho nem deixa a unidade presa; a posse guardada no disco permite concluir, depois do
# grace, que o dono não volta, e o trabalho é reoferecido.
#
# Sequência (a interseção de I1 com a tolerância a falhas, que nenhum outro roteiro cobria):
#   A) node-old reivindica uma unidade e a segura — o trace a mostra em voo.
#   B) node-fast conclui outras duas. É essa conclusão que carrega a posse de node-old para o
#      arquivo de estado: `claim` não grava, só uma mutação posterior leva o lease ao disco
#      (ver "Teto conhecido" em recovery.md).
#   C) kill no controller e em node-old, sem shutdown limpo.
#   D) o controller sobe do mesmo estado: a unidade volta EM VOO, ainda dona de node-old.
#   E) passado o grace sem o dono aparecer, a unidade volta para a fila.
#   F) node-new a reivindica, computa, e o cluster drena.
#
# O grace é medido a partir de quando o controller subiu (`started_unix_ms`), não do despacho
# original: o que importa é se o dono voltou depois do restart.
#
# A posse atravessando o restart tem teste unitário
# (`an_in_flight_unit_keeps_its_owner_across_a_restart`); o que este roteiro acrescenta é o
# caminho completo, com o dono ausente de verdade e a unidade terminando em outro nó.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051
STATE_DIR=/t/state-i4
STATE=$STATE_DIR/state.bin
# Folgado acima do intervalo de heartbeat de 5s, e bem abaixo do hold, para que o dono esteja
# saudável enquanto o controller está de pé e ainda assim não volte depois.
GRACE_MS=${GRACE_MS:-6000}
# Segura a unidade reivindicada: sem isso a janela em voo tem microssegundos e o kill cairia
# com o worker ocioso.
HOLD_MS=${HOLD_MS:-45000}

run() {
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

# Runs a command against the shared volume without joining the cluster network.
in_vol() {
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 --rm -v aic-target:/t "$IMG" sh -c "$1"
}

worker() {
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name "$1" --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID="$2" \
    -e AI_CLUSTER_WORK_DELAY_MS="$3" \
    "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
}

start_controller() {
  # Um kill deixa o container parado segurando o nome; o restart precisa do nome livre.
  docker rm -f aic-controller >/dev/null 2>&1
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-controller --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_STATE=$STATE -e AI_CLUSTER_HEARTBEAT_GRACE_MS=$GRACE_MS \
    "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
}

teardown() {
  docker rm -f aic-controller aic-worker-old aic-worker-fast aic-worker-new >/dev/null 2>&1
}

# O id de requisição que o enfileiramento imprimiu.
request_id_of() {
  sed -n 's/.*requisicao=\([^ ]*\).*/\1/p' | head -1
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null
# O controller escreve dentro do diretório, não o cria.
in_vol "rm -rf $STATE_DIR && mkdir -p $STATE_DIR"

echo "=== A) node-old reivindica uma unidade e a segura ==="
start_controller
worker aic-worker-old node-old "$HOLD_MS"
sleep 8

# O cliente do benchmark só volta quando a fila drena, e node-old segura a unidade pelo hold
# inteiro: ele fica em segundo plano e é cortado junto com o controller. O que importa aqui é
# o enfileiramento e a reivindicação, não o desfecho dele.
OUT=$(mktemp)
run "$R/aic cluster benchmark --size 64 --repetitions 1" >"$OUT" 2>&1 &
CLI=$!
sleep 10
REQ=$(request_id_of <"$OUT")
[ -n "$REQ" ] || { echo "FALHA: o benchmark não imprimiu requisicao="; cat "$OUT"; teardown; exit 1; }
echo "requisicao=$REQ"

INFLIGHT=$(run "$R/aic cluster request trace $REQ")
echo "$INFLIGHT"
if echo "$INFLIGHT" | grep -q "dispatched" && echo "$INFLIGHT" | grep -q "node-old"; then
  echo "OK: a unidade está em voo em node-old"
else
  echo "FALHA: esperado um estado 'dispatched' em node-old"
  teardown
  exit 1
fi

echo "=== B) node-fast conclui outras duas, levando a posse para o disco ==="
worker aic-worker-fast node-fast 0
sleep 3
# Em segundo plano pela mesma razão da seção A: o cliente espera a fila *inteira* drenar, e
# w-0 continua retida por node-old. Só as conclusões de node-fast interessam aqui.
OUT2=$(mktemp)
run "$R/aic cluster benchmark --size 64 --repetitions 2" >"$OUT2" 2>&1 &
CLI2=$!
for _ in $(seq 1 10); do
  [ "$(docker logs aic-controller 2>&1 | grep -c 'finalizado em node-fast')" = "2" ] && break
  sleep 2
done
DONE_FAST=$(docker logs aic-controller 2>&1 | grep -c "finalizado em node-fast")
echo "conclusoes em node-fast: $DONE_FAST"
if [ "$DONE_FAST" = "2" ]; then
  echo "OK: node-fast concluiu as duas; a gravação levou o lease de node-old ao disco"
else
  echo "FALHA: esperado 2 conclusões em node-fast, veio $DONE_FAST"
  teardown
  exit 1
fi

echo "=== C) kill no controller e em node-old ==="
docker kill aic-controller aic-worker-old >/dev/null 2>&1
wait "$CLI" 2>/dev/null || true
wait "$CLI2" 2>/dev/null || true

echo "=== D) o controller sobe do mesmo estado ==="
start_controller
sleep 3
LOADED=$(docker logs aic-controller 2>&1 | grep "estado carregado")
echo "$LOADED"
if echo "$LOADED" | grep -q "1 em voo"; then
  echo "OK: a unidade em voo voltou do disco, ainda dona de node-old"
else
  echo "FALHA: esperado '1 em voo' no carregamento do estado"
  teardown
  exit 1
fi

echo "=== E) passado o grace, a unidade volta para a fila ==="
sleep 8
RESTART_LOG=$(docker logs aic-controller 2>&1)
echo "$RESTART_LOG" | grep -E "não voltou|offline" || true
if echo "$RESTART_LOG" | grep -q "node-old não voltou após o restart"; then
  echo "OK: o dono ausente foi detectado depois do restart"
else
  echo "FALHA: esperado 'node-old não voltou após o restart' no log"
  teardown
  exit 1
fi

echo "=== F) node-new termina o trabalho ==="
worker aic-worker-new node-new 0
sleep 14

METRICS=$(run "$R/aic cluster metrics")
echo "$METRICS"
PENDING=$(echo "$METRICS" | sed -n 's/.*units_pending=\([0-9]*\).*/\1/p' | head -1)
FAILED=$(echo "$METRICS" | sed -n 's/.*units_failed=\([0-9]*\).*/\1/p' | head -1)
if [ "$PENDING" = "0" ] && [ "$FAILED" = "0" ]; then
  echo "OK: o cluster drenou (pendentes=0 falharam=0)"
else
  echo "FALHA: esperado pendentes=0 falharam=0, veio pendentes=$PENDING falharam=$FAILED"
  docker logs aic-controller 2>&1 | tail -20
  teardown
  exit 1
fi

FINAL=$(run "$R/aic cluster request trace $REQ")
echo "$FINAL"
if echo "$FINAL" | grep -q "completed" && echo "$FINAL" | grep -q "node-new"; then
  echo "OK: a unidade recuperada foi concluída em node-new, não no dono perdido"
else
  echo "FALHA: esperado 'completed' em node-new"
  teardown
  exit 1
fi

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== I4 OK ==="
