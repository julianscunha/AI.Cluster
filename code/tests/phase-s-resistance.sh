#!/usr/bin/env bash
# Resistência (F19): carga contínua com falhas no meio e um soak medindo a memória.
#
#   bash code/tests/phase-s-resistance.sh [minutos-de-soak, padrão 5]
#
# Sobe um controller (com estado em disco) e dois workers em contêineres, gera carga sem parar
# (grafos de multiplicação de matrizes) e:
#   A) aquece por 60 s e mede a memória de base do controller e dos workers.
#   B) partição de rede: desconecta um worker da rede por 25 s e reconecta; ele volta a READY e o
#      trabalho segue concluindo.
#   C) controller reiniciado sob carga: SIGTERM (desligamento limpo, grava o estado) e depois
#      `docker kill` (abrupto); depois de cada um o trabalho volta a concluir e a fila não cresce
#      sem limite.
#   D) soak até completar o tempo pedido, com amostra de memória a cada 30 s; falha se a memória
#      do controller ou de um worker cresce mais de 60 MiB (e mais de 50%) sobre a base.
#
# Precisa dos binários de release no volume aic-target (cargo build --release no aic-rust:1).
set -uo pipefail
export MSYS_NO_PATHCONV=1

SOAK_MIN=${1:-5}
NET=aic-net-s
IMG=aic-rust:1
R=/t/release
STATE_DIR=/t/state-s
CTRL=aic-controller-s
CONTROLLER=http://$CTRL:50051
TOKENS="-e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=s-join -e AI_CLUSTER_OPERATOR_TOKEN=s-op -e AI_CLUSTER_AUTO_APPROVE=1"

teardown() {
  docker rm -f $CTRL aic-worker-s1 aic-worker-s2 aic-load-s1 aic-load-s2 >/dev/null 2>&1
  docker network rm "$NET" >/dev/null 2>&1
}
fail() {
  echo "FALHA: $1"
  echo "--- controller"; docker logs --tail 10 $CTRL 2>&1 | cut -c1-200
  for w in aic-worker-s1 aic-worker-s2; do echo "--- $w"; docker logs --tail 5 $w 2>&1 | cut -c1-200; done
  teardown
  exit 1
}

cli() {
  docker run $TOKENS --rm --network "$NET" -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}
field() { grep -o "$1=[0-9]*" | head -1 | cut -d= -f2; }
completed() { cli "$R/aic cluster metrics" 2>/dev/null | field units_completed; }
pending() { cli "$R/aic cluster metrics" 2>/dev/null | field units_pending; }
# Memória em MiB de um contêiner (docker stats traz "12.3MiB / 3.8GiB").
mem() {
  docker stats --no-stream --format '{{.MemUsage}}' "$1" 2>/dev/null | awk '{
    v=$1; if (v ~ /GiB/) {sub(/GiB/,"",v); print int(v*1024)} else if (v ~ /KiB/) {sub(/KiB/,"",v); print int(v/1024)} else {sub(/MiB/,"",v); print int(v)} }'
}

start_controller() {
  docker run $TOKENS -d --name $CTRL --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_STATE=$STATE_DIR/state.bin "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
}
start_worker() {
  docker run $TOKENS -d --name "$1" --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID="$2" "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
}
start_load() {
  docker run $TOKENS -d --name "$1" --network "$NET" -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" \
    sh -c "while true; do $R/aic cluster graph --size 64 >/dev/null 2>&1 || sleep 1; done" >/dev/null
}
# Espera o contador de concluídas subir $2 unidades em até $3 s.
progress() {
  local label=$1 want=$2 limit=$3 before now
  before=$(completed); before=${before:-0}
  for _ in $(seq 1 "$limit"); do
    now=$(completed); now=${now:-0}
    [ $((now - before)) -ge "$want" ] && { echo "OK: $label (+$((now - before)) unidades concluídas)"; return 0; }
    sleep 2
  done
  return 1
}

trap teardown EXIT INT TERM
teardown
docker network create "$NET" >/dev/null
docker run $TOKENS --rm -v aic-target:/t "$IMG" sh -c "rm -rf $STATE_DIR && mkdir -p $STATE_DIR" >/dev/null

echo "=== A) subir o cluster e aquecer ==="
start_controller; sleep 4
start_worker aic-worker-s1 node-s1
start_worker aic-worker-s2 node-s2
sleep 6
start_load aic-load-s1
start_load aic-load-s2
progress "carga concluindo com 2 workers" 20 60 || fail "a carga não andou depois de subir o cluster"
sleep 60
BASE_C=$(mem $CTRL); BASE_W1=$(mem aic-worker-s1); BASE_W2=$(mem aic-worker-s2)
[ -n "$BASE_C" ] && [ -n "$BASE_W1" ] && [ -n "$BASE_W2" ] || fail "não consegui medir a memória de base"
echo "memória de base (MiB): controller=$BASE_C worker1=$BASE_W1 worker2=$BASE_W2"

echo "=== B) partição de rede de um worker ==="
docker network disconnect "$NET" aic-worker-s1
sleep 25
docker network connect "$NET" aic-worker-s1
OK=0
for _ in $(seq 1 30); do
  cli "$R/aic cluster nodes" 2>/dev/null | grep -E "node-s1 +READY" >/dev/null && { OK=1; break; }
  sleep 2
done
[ "$OK" = 1 ] && echo "OK: node-s1 voltou a READY depois da partição" || fail "node-s1 não voltou a READY"
progress "trabalho segue concluindo depois da partição" 20 60 || fail "a carga não andou depois da partição"

echo "=== C) controller reiniciado sob carga ==="
docker stop -t 10 $CTRL >/dev/null; docker rm -f $CTRL >/dev/null; start_controller; sleep 8
progress "depois do desligamento limpo (SIGTERM)" 20 90 || fail "a carga não voltou depois do SIGTERM"
docker kill $CTRL >/dev/null; docker rm -f $CTRL >/dev/null; start_controller; sleep 8
progress "depois do kill abrupto" 20 90 || fail "a carga não voltou depois do kill"
P=$(pending); echo "fila depois dos reinícios: pending=${P:-?}"
[ "${P:-9999}" -lt 200 ] || fail "a fila cresceu sem limite (pending=$P)"

echo "=== D) soak de $SOAK_MIN min com amostra de memória a cada 30 s ==="
END=$(( $(date +%s) + SOAK_MIN * 60 ))
MAX_C=$BASE_C; MAX_W1=$BASE_W1; MAX_W2=$BASE_W2
while [ "$(date +%s)" -lt "$END" ]; do
  sleep 30
  C=$(mem $CTRL); W1=$(mem aic-worker-s1); W2=$(mem aic-worker-s2)
  [ "${C:-0}" -gt "$MAX_C" ] && MAX_C=$C
  [ "${W1:-0}" -gt "$MAX_W1" ] && MAX_W1=$W1
  [ "${W2:-0}" -gt "$MAX_W2" ] && MAX_W2=$W2
  echo "  t=$(date +%T) controller=${C}MiB worker1=${W1}MiB worker2=${W2}MiB concluídas=$(completed) fila=$(pending)"
done
progress "trabalho ainda concluindo no fim do soak" 10 60 || fail "a carga parou durante o soak"

# Crescimento aceito: até 60 MiB ou 50% sobre a base, o que for maior (alocadores e caches enchem).
grows() {
  local name=$1 base=$2 peak=$3 limit
  limit=$(( base + 60 )); [ $(( base * 3 / 2 )) -gt "$limit" ] && limit=$(( base * 3 / 2 ))
  if [ "$peak" -gt "$limit" ]; then
    echo "FALHA: $name cresceu de ${base} para ${peak} MiB (limite $limit)"; return 1
  fi
  echo "OK: $name ${base} -> ${peak} MiB (limite $limit)"
}
RC=0
grows controller "$BASE_C" "$MAX_C" || RC=1
grows worker1 "$BASE_W1" "$MAX_W1" || RC=1
grows worker2 "$BASE_W2" "$MAX_W2" || RC=1
cli "$R/aic cluster metrics" 2>/dev/null | grep -E "units_(pending|completed|failed)" | head -3
teardown
[ "$RC" = 0 ] && echo "=== resistência OK ===" || { echo "=== resistência FALHOU ==="; exit 1; }
