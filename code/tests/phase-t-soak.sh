#!/usr/bin/env bash
# Fatia da F19: soak curto com injeção de falhas sistemática.
#
# Critério verificado: sob falhas repetidas e alternadas, todo lote termina inteiro, sem unidade
# perdida nem duplicada, e o controller segue de pé com o cluster completo no fim.
# A cada rodada um lote de unidades corre enquanto um worker, em rodízio, sofre uma falha:
#   kill  = SIGKILL, sem leave (o controller só percebe pela graça do heartbeat);
#   pause = congelado (heartbeat para, o processo continua segurando a unidade) e depois retomado,
#           o que exercita o resultado tardio de uma unidade já reenfileirada.
# ROUNDS controla a duração (padrão 6 rodadas, ~3 min); para um soak longo use ROUNDS=200.
set -uo pipefail

NET=aic-net-soak
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller-soak:50051
ROUNDS=${ROUNDS:-6}
UNITS=${UNITS:-6}
WORKERS=(a b c)
ENVS="-e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1"

run() {
  MSYS_NO_PATHCONV=1 docker run $ENVS --rm --network "$NET" -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

start_worker() {
  docker rm -f "aic-soak-$1" >/dev/null 2>&1
  MSYS_NO_PATHCONV=1 docker run $ENVS -d --name "aic-soak-$1" --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID="node-soak-$1" \
    -e AI_CLUSTER_WORK_DELAY_MS=4000 "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
}

teardown() {
  docker unpause aic-soak-a aic-soak-b aic-soak-c >/dev/null 2>&1
  docker rm -f aic-controller-soak aic-soak-a aic-soak-b aic-soak-c >/dev/null 2>&1
  docker network rm "$NET" >/dev/null 2>&1
}

fail() {
  echo "FALHA: $1"
  docker logs aic-controller-soak 2>&1 | tail -20
  for w in "${WORKERS[@]}"; do echo "--- worker $w ---"; docker logs "aic-soak-$w" 2>&1 | tail -8; done
  teardown
  exit 1
}

teardown
docker network create "$NET" >/dev/null
MSYS_NO_PATHCONV=1 docker run $ENVS -d --name aic-controller-soak --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_HEARTBEAT_GRACE_MS=6000 "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
for w in "${WORKERS[@]}"; do start_worker "$w"; done
sleep 10

for i in $(seq 1 "$ROUNDS"); do
  victim=${WORKERS[$(( (i - 1) % 3 ))]}
  mode=kill
  [ $((i % 2)) -eq 0 ] && mode=pause
  echo "=== rodada $i/$ROUNDS: $mode em node-soak-$victim ==="
  OUT=$(mktemp)
  run "$R/ai-cluster benchmark --size 32 --repetitions $UNITS" >"$OUT" 2>&1 &
  CLI=$!
  sleep 4
  if [ "$mode" = kill ]; then
    docker kill "aic-soak-$victim" >/dev/null 2>&1
  else
    docker pause "aic-soak-$victim" >/dev/null 2>&1
    sleep 9
    docker unpause "aic-soak-$victim" >/dev/null 2>&1
  fi
  wait "$CLI"
  grep concluidas "$OUT"
  # The CLI summary is cumulative over the controller's life, so round i ends at UNITS * i.
  grep -q "concluidas=$((UNITS * i)) pendentes=0 falharam=0" "$OUT" || { cat "$OUT"; fail "rodada $i: lote incompleto"; }
  rm -f "$OUT"
  [ "$mode" = kill ] && start_worker "$victim"
  sleep 10
  [ "$(docker inspect -f '{{.State.Running}}' aic-controller-soak)" = true ] || fail "controller caiu na rodada $i"
done

echo "=== estado final ==="
NODES=$(run "$R/ai-cluster nodes")
echo "$NODES"
READY=$(echo "$NODES" | grep -c "node-soak-.* READY")
[ "$READY" -eq 3 ] || fail "esperados 3 nós READY no fim, vieram $READY"
EVENTS=$(run "$R/ai-cluster events --limit 1000")
OFFLINE=$(echo "$EVENTS" | grep -c node_offline)
[ "$OFFLINE" -ge 1 ] || fail "nenhum node_offline registrado"
REQUEUED=$(docker logs aic-controller-soak 2>&1 | grep -oE "[0-9]+ unidades em voo reenfileiradas" | awk '{ n += $1 } END { print n + 0 }')
[ "$REQUEUED" -ge 1 ] || fail "nenhuma unidade em voo foi reenfileirada: a falha não pegou trabalho em andamento"
echo "OK: $ROUNDS rodadas de falha, todos os lotes completos, 3 nós READY, $OFFLINE node_offline, $REQUEUED unidades reenfileiradas"

teardown
echo "=== fim ==="
