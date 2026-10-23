#!/usr/bin/env bash
# Fatia da Fase H (H5): previsto vs real — o erro do modelo de custo.
#
# Critério verificado: o trace não mostra só o que a unidade custou, mas o que o modelo de
# custo tinha previsto, e a distância entre os dois. Um nó sem compute score não prevê nada,
# e zero ali é lido como "não previsto", não como "instantâneo".
#
# O nó reporta um compute score fixo (AI_CLUSTER_COMPUTE_SCORE=1.0) em 4 threads, então a
# previsão é aritmética conhecida: previsto_us = 2*size^3 / (4 * 1.0 * 6.0 * 1e3) + setup,
# com o 6.0 vindo de REFERENCE_CORE_GFLOPS e o setup sendo `AI_CLUSTER_UNIT_SETUP_US`
# (UNIT_SETUP_US por padrão). Para size=64 dá 22 + 600 = 622us por unidade; para size=128,
# 175 + 600 = 775us.
#
# Sequência:
#   A) size=64: o trace mostra PREDITO_US=622 e PREVISTO somado_us=1244, com erro_pct coerente.
#   B) size=128: a previsão escala com o cubo do tamanho (775 ≈ 8 * 22 + setup).
#   C) uma requisição desconhecida não imprime linha PREVISTO (nada foi previsto).
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051
SCORE=1.0
THREADS=4

run() {
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

teardown() {
  docker rm -f aic-controller aic-worker-p >/dev/null 2>&1
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -d --name aic-controller --network "$NET" -v aic-target:/t \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -d --name aic-worker-p --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-p \
  -e AI_CLUSTER_COMPUTE_SCORE=$SCORE -e AI_CLUSTER_CPU_THREADS=$THREADS \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 8

echo "=== A) size=64 — previsão de 622us por unidade ==="
OUT=$(run "$R/ai-cluster benchmark --size 64 --repetitions 2")
echo "$OUT"
REQ=$(echo "$OUT" | sed -n 's/.*requisicao=\([^ ]*\).*/\1/p' | head -1)
[ -n "$REQ" ] || { echo "FALHA: o benchmark não imprimiu requisicao="; teardown; exit 1; }
TRACE=$(run "$R/ai-cluster request trace $REQ")
echo "$TRACE"
PREDITO=$(echo "$TRACE" | sed -n 's/^PREVISTO somado_us=\([0-9]*\).*/\1/p')
if [ "$PREDITO" = "1244" ]; then
  echo "OK: PREVISTO somado_us=1244 (2 unidades de 64³ a 622us cada)"
else
  echo "FALHA: PREVISTO somado_us esperado 1244, veio '$PREDITO'"
  teardown
  exit 1
fi
# Cada linha de unidade concluída carrega a previsão ao lado do real.
PRED_ROWS=$(echo "$TRACE" | grep -c "         622 ")
if [ "$PRED_ROWS" = "2" ]; then
  echo "OK: as 2 linhas mostram PREDITO_US=622"
else
  echo "FALHA: esperado 2 linhas com PREDITO_US=622, veio $PRED_ROWS"
  teardown
  exit 1
fi
# erro_pct = (real - previsto) / previsto * 100. O real é do relógio, então não dá para
# fixar o valor. Antes do termo de setup o modelo era ~30x otimista (erro ~3000%); agora
# ele pode errar para os dois lados — um nó ocupado faz o real ficar acima, um nó ocioso
# abaixo — mas não por ordens de magnitude. Um teto de 1000% é o que ainda separa "modelo
# calibrado" de "modelo otimista", com folga para o ruído do host compartilhado.
ERR=$(echo "$TRACE" | sed -n 's/.*erro_pct=\(-\?[0-9.]*\).*/\1/p')
if [ -n "$ERR" ] && awk -v e="$ERR" 'BEGIN{exit !((e < 0 ? -e : e) < 1000)}'; then
  echo "OK: erro_pct=$ERR dentro do teto de 1000% (o modelo já não é otimista por 30x)"
else
  echo "FALHA: |erro_pct| deveria ser < 1000, veio '$ERR'"
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
if [ "$PREDITO2" = "775" ]; then
  echo "OK: PREVISTO somado_us=775 (128³ é 8x o compute de 64³, mais o setup)"
else
  echo "FALHA: PREVISTO somado_us esperado 775, veio '$PREDITO2'"
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
if [ "$PRED_TOTAL" = "2019" ]; then
  echo "OK: total_predicted_compute_us=2019 (1244 do A + 775 do B)"
else
  echo "FALHA: esperado total_predicted_compute_us=2019, veio '$PRED_TOTAL'"
  teardown
  exit 1
fi

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== H5 OK ==="
