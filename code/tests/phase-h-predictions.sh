#!/usr/bin/env bash
# Fatia da Fase H (H5): previsto vs real — o erro do modelo de custo.
#
# Critério verificado: o trace não mostra só o que a unidade custou, mas o que o modelo de
# custo tinha previsto, e a distância entre os dois. Um nó sem compute score não prevê nada,
# e zero ali é lido como "não previsto", não como "instantâneo".
#
# O nó reporta um compute score fixo (AI_CLUSTER_COMPUTE_SCORE=1.0) em 4 threads, então a
# previsão é aritmética conhecida: previsto_us = 2*size^3 / (4 * 1.0 * 6.0 * 1e3), com o
# 6.0 vindo de REFERENCE_CORE_GFLOPS. Para size=64 dá 22us por unidade; para size=128, 175us.
#
# Sequência:
#   A) size=64: o trace mostra PREDITO_US=22 e PREVISTO somado_us=44, com erro_pct coerente.
#   B) size=128: a previsão escala com o cubo do tamanho (175 ≈ 8 * 22).
#   C) uma requisição desconhecida não imprime linha PREVISTO (nada foi previsto).
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051
SCORE=1.0
THREADS=4

run() {
  MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

teardown() {
  docker rm -f aic-controller aic-worker-p >/dev/null 2>&1
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

MSYS_NO_PATHCONV=1 docker run -d --name aic-controller --network "$NET" -v aic-target:/t \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
MSYS_NO_PATHCONV=1 docker run -d --name aic-worker-p --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-p \
  -e AI_CLUSTER_COMPUTE_SCORE=$SCORE -e AI_CLUSTER_CPU_THREADS=$THREADS \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 8

echo "=== A) size=64 — previsão de 22us por unidade ==="
OUT=$(run "$R/ai-cluster benchmark --size 64 --repetitions 2")
echo "$OUT"
REQ=$(echo "$OUT" | sed -n 's/.*requisicao=\([^ ]*\).*/\1/p' | head -1)
[ -n "$REQ" ] || { echo "FALHA: o benchmark não imprimiu requisicao="; teardown; exit 1; }
TRACE=$(run "$R/ai-cluster request trace $REQ")
echo "$TRACE"
PREDITO=$(echo "$TRACE" | sed -n 's/^PREVISTO somado_us=\([0-9]*\).*/\1/p')
if [ "$PREDITO" = "44" ]; then
  echo "OK: PREVISTO somado_us=44 (2 unidades de 64³ a 22us cada)"
else
  echo "FALHA: PREVISTO somado_us esperado 44, veio '$PREDITO'"
  teardown
  exit 1
fi
# Cada linha de unidade concluída carrega a previsão ao lado do real.
PRED_ROWS=$(echo "$TRACE" | grep -c "         22 ")
if [ "$PRED_ROWS" = "2" ]; then
  echo "OK: as 2 linhas mostram PREDITO_US=22"
else
  echo "FALHA: esperado 2 linhas com PREDITO_US=22, veio $PRED_ROWS"
  teardown
  exit 1
fi
# erro_pct = (real - previsto) / previsto * 100. O real é do relógio, então não dá para
# fixar o valor; mas tem que ser positivo — o modelo prevê menos do que a máquina gasta.
ERR=$(echo "$TRACE" | sed -n 's/.*erro_pct=\(-\?[0-9.]*\).*/\1/p')
if [ -n "$ERR" ] && awk "BEGIN{exit !($ERR > 0)}"; then
  echo "OK: erro_pct=$ERR > 0 (o modelo subestima o compute, como esperado)"
else
  echo "FALHA: erro_pct deveria ser > 0, veio '$ERR'"
  teardown
  exit 1
fi

echo "=== B) size=128 — a previsão escala com o cubo ==="
OUT2=$(run "$R/ai-cluster benchmark --size 128 --repetitions 1")
echo "$OUT2"
REQ2=$(echo "$OUT2" | sed -n 's/.*requisicao=\([^ ]*\).*/\1/p' | head -1)
TRACE2=$(run "$R/ai-cluster request trace $REQ2")
echo "$TRACE2"
PREDITO2=$(echo "$TRACE2" | sed -n 's/^PREVISTO somado_us=\([0-9]*\).*/\1/p')
if [ "$PREDITO2" = "175" ]; then
  echo "OK: PREVISTO somado_us=175 (128³ é 8x o trabalho de 64³)"
else
  echo "FALHA: PREVISTO somado_us esperado 175, veio '$PREDITO2'"
  teardown
  exit 1
fi

echo "=== C) requisição desconhecida — nada previsto ==="
UNKNOWN=$(run "$R/ai-cluster request trace nao-existe")
echo "$UNKNOWN"
if echo "$UNKNOWN" | grep -q "PREVISTO"; then
  echo "FALHA: trace vazio não deveria imprimir linha PREVISTO"
  teardown
  exit 1
fi
echo "OK: sem linha PREVISTO quando não há previsão"

echo "=== D) roll-up do cluster — a previsão somada no metrics ==="
METRICS=$(run "$R/ai-cluster metrics")
echo "$METRICS"
PRED_TOTAL=$(echo "$METRICS" | grep -o "total_predicted_compute_us=[0-9]*" | head -1 | cut -d= -f2)
if [ "$PRED_TOTAL" = "219" ]; then
  echo "OK: total_predicted_compute_us=219 (44 do A + 175 do B)"
else
  echo "FALHA: esperado total_predicted_compute_us=219, veio '$PRED_TOTAL'"
  teardown
  exit 1
fi

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== H5 OK ==="
