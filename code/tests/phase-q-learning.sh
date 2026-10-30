#!/usr/bin/env bash
# Fatia da Fase Q (F17, P1): o modelo de custo aprende a velocidade real de cada nó.
#
# Critério verificado: um nó que anuncia um compute score 4x acima do que entrega (fixado de
# propósito) começa com previsões muito erradas, e depois de algumas unidades o erro cai: a
# previsão de cada nó passa a ser corrigida pela média móvel do real sobre o previsto.
set -uo pipefail

NET=aic-net-q
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller-q:50051
ENVS="-e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=q-join -e AI_CLUSTER_OPERATOR_TOKEN=q-operator -e AI_CLUSTER_AUTO_APPROVE=1"
run() { MSYS_NO_PATHCONV=1 docker run $ENVS --rm --network "$NET" -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"; }
teardown() { docker rm -f aic-controller-q aic-worker-q >/dev/null 2>&1; }
fail() { echo "FALHA: $1"; teardown; exit 1; }

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null
MSYS_NO_PATHCONV=1 docker run -d --name aic-controller-q $ENVS --network "$NET" -v aic-target:/t "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
MSYS_NO_PATHCONV=1 docker run -d --name aic-worker-q $ENVS --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-q \
  -e AI_CLUSTER_COMPUTE_SCORE=4.0 -e AI_CLUSTER_CPU_THREADS=1 "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 8

echo "=== 40 unidades, uma por vez, num nó que promete 4x o que entrega ==="
OUT=$(run "$R/aic cluster benchmark --size 64 --repetitions 40")
REQ=$(echo "$OUT" | sed -n 's/.*requisicao=\([^ ]*\).*/\1/p' | head -1)
[ -n "$REQ" ] || fail "sem request id"
TRACE=$(run "$R/aic cluster request trace $REQ")
# Linhas "w-N nó completed COMPUTE PREDITO ..."; ordena pelo número da unidade.
ROWS=$(echo "$TRACE" | awk '$3 == "completed" { sub("w-", "", $1); print $1, $4, $5 }' | sort -n)
echo "$ROWS" | head -3; echo "..."; echo "$ROWS" | tail -3
COUNT=$(echo "$ROWS" | wc -l)
[ "$COUNT" -ge 30 ] || fail "esperadas ao menos 30 unidades concluídas, vieram $COUNT"

# Erro relativo médio |real - previsto| / real: as 5 primeiras contra as 15 últimas.
err() { awk '$3 > 0 && $2 > 0 { d = $2 - $3; if (d < 0) d = -d; s += d / $2; n++ } END { if (n) printf "%.3f", s / n; else print "nan" }'; }
FIRST=$(echo "$ROWS" | head -5 | err)
LAST=$(echo "$ROWS" | tail -15 | err)
echo "erro relativo médio: primeiras 5 = $FIRST, últimas 15 = $LAST"
awk -v a="$FIRST" -v b="$LAST" 'BEGIN { exit !(a > 0.5 && b < a / 2) }' \
  || fail "o erro deveria cair a menos da metade (primeiras $FIRST, últimas $LAST)"
echo "OK: o erro da previsão caiu de $FIRST para $LAST"

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== fim ==="
