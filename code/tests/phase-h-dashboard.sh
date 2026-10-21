#!/usr/bin/env bash
# Fatia da Fase H (H4): o painel de uma tela.
#
# Critério verificado: o painel lê o estado do cluster e reflete o que muda — nó
# disponível, trabalho concluído e reserva ativa — numa vista só. O painel não inventa
# número: cada seção vem de uma chamada que o controlador já expõe (`ListNodes`,
# `GetMetrics`, `ListReservations`), então o teste é que as três seções aparecem e
# acompanham o cluster.
#
# Sequência:
#   A) cluster ocioso: 1 nó disponível, nada concluído, nenhuma reserva.
#   B) depois de 2 unidades: concluídas=2 no painel.
#   C) com uma reserva ativa: RESERVAS 1 e o nó retido aparecem.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051
SIZE=64

run() {
  MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

teardown() {
  docker rm -f aic-controller aic-worker-d >/dev/null 2>&1
}

# expect_contains <rótulo> <texto esperado> <saída do painel>
expect_contains() {
  if grep -qF "$2" <<<"$3"; then
    echo "OK: $1 — encontrou '$2'"
  else
    echo "FALHA: $1 — esperava '$2'"
    echo "--- painel ---"
    echo "$3"
    teardown
    exit 1
  fi
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

MSYS_NO_PATHCONV=1 docker run -d --name aic-controller --network "$NET" -v aic-target:/t \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
MSYS_NO_PATHCONV=1 docker run -d --name aic-worker-d --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-d \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 8

echo "=== A) cluster ocioso ==="
PANEL=$(run "$R/ai-cluster dashboard")
echo "$PANEL"
expect_contains "cabeçalho" "PAINEL nós_disponíveis=1/1" "$PANEL"
expect_contains "nó listado" "node-d" "$PANEL"
expect_contains "nada concluído" "TRABALHO concluídas=0" "$PANEL"
expect_contains "sem reservas" "RESERVAS 0" "$PANEL"

echo "=== B) duas unidades executadas ==="
run "$R/ai-cluster benchmark --size $SIZE --repetitions 2" >/dev/null
PANEL=$(run "$R/ai-cluster dashboard")
echo "$PANEL"
expect_contains "concluídas somam" "TRABALHO concluídas=2" "$PANEL"
expect_contains "tempo registrado" "TEMPO compute_total_us=" "$PANEL"

echo "=== C) reserva ativa ==="
run "$R/ai-cluster reserve node-d req-panel --ttl-ms 60000" >/dev/null
PANEL=$(run "$R/ai-cluster dashboard")
echo "$PANEL"
expect_contains "reserva contada" "RESERVAS 1" "$PANEL"
expect_contains "requisição dona" "requisição=req-panel" "$PANEL"

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== H4 OK ==="
