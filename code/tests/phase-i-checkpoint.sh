#!/usr/bin/env bash
# Fatia da Fase I (I3): o checkpoint não carrega o que ninguém vai ler.
#
# Critério verificado: o estado salvo é proporcional ao trabalho *que falta*, não ao trabalho
# já feito. Antes desta fatia cada unidade concluída deixava a sua matriz no snapshot e o
# snapshot inteiro era reescrito a cada conclusão — quadrático no trabalho terminado. Medido
# antes da mudança: 8 unidades de 256³ deixavam 4.194.410 bytes em `state.bin`, que é
# exatamente 8 × 256 × 256 × 8: só as matrizes de saída, nada mais.
#
# Sequência:
#   A) 8 unidades de 256³ com estado ligado, sem dependências entre elas.
#   B) o snapshot final é menor que UMA matriz de saída: nenhuma sobrou.
#   C) um controller novo sobre o mesmo arquivo carrega sem erro e sem pendências — a poda
#      não pode ter corrompido nem ressuscitado trabalho.
#
# A poda em si — o que fica e o que sai, pelos dois caminhos de leitura (dependência e entrada
# vinculada) — é provada por teste unitário em
# `an_output_is_dropped_once_nothing_can_read_it` e
# `an_output_an_unfinished_unit_still_reads_is_kept` (code/crates/controller/src/work.rs).
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051
STATE_DIR=/t/state-i3
STATE=$STATE_DIR/state.bin
# 256 × 256 × 8 bytes: o tamanho de uma única matriz de saída em f64.
ONE_MATRIX=524288

run() {
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

teardown() {
  docker rm -f aic-controller aic-worker-i3 >/dev/null 2>&1
}

field() {
  grep -o "$1=[0-9]*" | head -1 | cut -d= -f2
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} --rm -v aic-target:/t "$IMG" sh -c "rm -rf $STATE_DIR && mkdir -p $STATE_DIR"

MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -d --name aic-controller --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_STATE=$STATE "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -d --name aic-worker-i3 --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-i3 \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 6

echo "=== A) 8 unidades de 256³, sem dependências ==="
run "$R/ai-cluster benchmark --size 256 --repetitions 8" 2>&1 | tail -2

echo "=== B) o snapshot final não guarda nenhuma matriz de saída ==="
SIZE=$(MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} --rm -v aic-target:/t "$IMG" sh -c "stat -c %s $STATE")
echo "state.bin = $SIZE bytes (uma matriz de saída = $ONE_MATRIX bytes)"
if [ "$SIZE" -lt "$ONE_MATRIX" ] 2>/dev/null; then
  echo "OK: 8 unidades concluídas e o estado é menor que uma única matriz"
else
  echo "FALHA: o estado guardou as matrizes de saída ($SIZE bytes)"
  teardown
  exit 1
fi
if MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} --rm -v aic-target:/t "$IMG" sh -c "test -s $STATE.replica"; then
  echo "OK: a cópia também foi escrita"
else
  echo "FALHA: a cópia está ausente ou vazia"
  teardown
  exit 1
fi

echo "=== C) reload sobre o mesmo arquivo ==="
docker kill aic-controller >/dev/null 2>&1
docker rm -f aic-controller >/dev/null 2>&1
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -d --name aic-controller --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_STATE=$STATE "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
sleep 4
REPORT=$(run "$R/ai-cluster metrics" 2>/dev/null)
PEND=$(echo "$REPORT" | field units_pending)
if [ "$PEND" = "0" ]; then
  echo "OK: o checkpoint carregou e não ressuscitou trabalho (units_pending=0)"
else
  echo "FALHA: esperado units_pending=0, veio '$PEND'"
  teardown
  exit 1
fi

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== fim ==="