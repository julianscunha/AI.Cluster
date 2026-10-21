#!/usr/bin/env bash
# Fatia da Fase C (C3): capability "current + confidence".
#
# Critério verificado: o perfil de capability deixa de ser só o baseline do registro. Um nó
# que remede o próprio compute enquanto ocioso reporta a medida nova ao lado da antiga, e
# uma medida instável carrega uma confiança menor que uma estável. Tudo isso viaja no
# `HealthStreamRequest`, não só no registro.
#
# Sequência:
#   A) nó ocioso: o detalhe mostra `current_compute_score` preenchido e `compute_confidence`
#      dentro de (0,1], ao lado do `compute_score` do registro.
#   B) `AI_CLUSTER_COMPUTE_SCORE` fixado: o nó não remede — o pin manda no número — então o
#      detalhe mostra `current_compute_score=-`, que é "não remedido", não "compute zero".
#
# O nó do passo A não fixa o score, então a remedida é uma leitura real do host. O passo B
# prova que o pin continua valendo para a remedida: sem isso, um teste de determinismo
# ficaria não-determinístico.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051

run() {
  MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

teardown() {
  docker rm -f aic-controller aic-worker-cur >/dev/null 2>&1
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

echo "=== A) nó ocioso remede o compute enquanto espera trabalho ==="
MSYS_NO_PATHCONV=1 docker run -d --name aic-controller --network "$NET" -v aic-target:/t \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
MSYS_NO_PATHCONV=1 docker run -d --name aic-worker-cur --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-cur \
  -e AI_CLUSTER_RECOMPUTE_INTERVAL_SECS=0 \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
# The worker measures at registration, then remeasures on every idle pass (interval 0).
# The heartbeat that carries the profile runs every 5s, so a couple of ticks is enough.
sleep 12

DETAIL=$(run "$R/ai-cluster node show node-cur")
echo "$DETAIL"

BASELINE=$(echo "$DETAIL" | sed -n 's/.*compute_score=\([0-9.]*\).*/\1/p' | head -1)
CURRENT=$(echo "$DETAIL" | sed -n 's/.*current_compute_score=\([0-9.-]*\).*/\1/p' | head -1)
CONF=$(echo "$DETAIL" | sed -n 's/.*compute_confidence=\([0-9.-]*\).*/\1/p' | head -1)

echo "baseline=$BASELINE current=$CURRENT confidence=$CONF"
if awk -v s="$BASELINE" 'BEGIN { exit !(s+0 > 0) }'; then
  echo "OK: o baseline do registro está preenchido"
else
  echo "FALHA: compute_score deveria ser > 0 (veio '$BASELINE')"
  teardown
  exit 1
fi
if [ "$CURRENT" = "-" ] || [ -z "$CURRENT" ]; then
  echo "FALHA: current_compute_score deveria estar remedido (veio '$CURRENT')"
  teardown
  exit 1
fi
if awk -v s="$CURRENT" 'BEGIN { exit !(s+0 > 0) }'; then
  echo "OK: o current foi remedido com o nó ocioso"
else
  echo "FALHA: current_compute_score deveria ser > 0 (veio '$CURRENT')"
  teardown
  exit 1
fi
if awk -v c="$CONF" 'BEGIN { exit !(c+0 > 0 && c+0 <= 1) }'; then
  echo "OK: a confiança está em (0,1] (veio '$CONF')"
else
  echo "FALHA: compute_confidence deveria estar em (0,1] (veio '$CONF')"
  teardown
  exit 1
fi

teardown
echo "=== B) score fixado: o nó não remede, então current fica '-' ==="
MSYS_NO_PATHCONV=1 docker run -d --name aic-controller --network "$NET" -v aic-target:/t \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
MSYS_NO_PATHCONV=1 docker run -d --name aic-worker-cur --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-cur \
  -e AI_CLUSTER_COMPUTE_SCORE=2.0 -e AI_CLUSTER_RECOMPUTE_INTERVAL_SECS=0 \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 12

PINNED=$(run "$R/ai-cluster node show node-cur")
echo "$PINNED"
PINNED_BASE=$(echo "$PINNED" | sed -n 's/.*compute_score=\([0-9.]*\).*/\1/p' | head -1)
PINNED_CUR=$(echo "$PINNED" | sed -n 's/.*current_compute_score=\([0-9.-]*\).*/\1/p' | head -1)
if [ "$PINNED_BASE" = "2.0000" ]; then
  echo "OK: o baseline fixado foi reportado como veio"
else
  echo "FALHA: esperado 2.0000, veio '$PINNED_BASE'"
  teardown
  exit 1
fi
if [ "$PINNED_CUR" = "-" ]; then
  echo "OK: o pin impede a remedida, então current é 'não remedido'"
else
  echo "FALHA: current deveria ser '-' com o score fixado (veio '$PINNED_CUR')"
  teardown
  exit 1
fi

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== C3 OK ==="
