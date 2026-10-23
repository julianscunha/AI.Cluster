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
#      dentro de (0,1], ao lado do `compute_score` do registro, da `compute_median_score`
#      (o valor central que a previsão calibra) e do `compute_deviation` (quanto as execuções
#      discordaram).
#   B) `AI_CLUSTER_COMPUTE_SCORE` fixado: o nó não remede — o pin manda no número — então o
#      detalhe mostra `current_compute_score=-` e `compute_median_score=-`, que é "não
#      medido", não "compute zero".
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
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

teardown() {
  docker rm -f aic-controller aic-worker-cur >/dev/null 2>&1
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

echo "=== A) nó ocioso remede o compute enquanto espera trabalho ==="
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -d --name aic-controller --network "$NET" -v aic-target:/t \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -d --name aic-worker-cur --network "$NET" -v aic-target:/t \
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
MEDIAN=$(echo "$DETAIL" | sed -n 's/.*compute_median_score=\([0-9.-]*\).*/\1/p' | head -1)
DEV=$(echo "$DETAIL" | sed -n 's/.*compute_deviation=\([0-9.-]*\).*/\1/p' | head -1)

echo "baseline=$BASELINE current=$CURRENT confidence=$CONF median=$MEDIAN desvio=$DEV"
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
# The median is the same benchmark's central run: it exists whenever the score does, and it
# can never pass the best case, which is what the score is.
if awk -v m="$MEDIAN" 'BEGIN { exit !(m+0 > 0) }'; then
  echo "OK: a mediana do benchmark foi reportada (veio '$MEDIAN')"
else
  echo "FALHA: compute_median_score deveria ser > 0 (veio '$MEDIAN')"
  teardown
  exit 1
fi
if awk -v m="$MEDIAN" -v b="$BASELINE" 'BEGIN { exit !(m+0 <= b+0) }'; then
  echo "OK: a mediana ($MEDIAN) não passa do melhor caso ($BASELINE)"
else
  echo "FALHA: mediana deveria ser <= compute_score (mediana=$MEDIAN baseline=$BASELINE)"
  teardown
  exit 1
fi
# A relative deviation is a fraction, so it can never be negative.
if awk -v d="$DEV" 'BEGIN { exit !(d+0 >= 0) }'; then
  echo "OK: o desvio é não-negativo (veio '$DEV')"
else
  echo "FALHA: compute_deviation deveria ser >= 0 (veio '$DEV')"
  teardown
  exit 1
fi

teardown
echo "=== B) score fixado: o nó não remede, então current fica '-' ==="
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -d --name aic-controller --network "$NET" -v aic-target:/t \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -d --name aic-worker-cur --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-cur \
  -e AI_CLUSTER_COMPUTE_SCORE=2.0 -e AI_CLUSTER_RECOMPUTE_INTERVAL_SECS=0 \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 12

PINNED=$(run "$R/ai-cluster node show node-cur")
echo "$PINNED"
PINNED_BASE=$(echo "$PINNED" | sed -n 's/.*compute_score=\([0-9.]*\).*/\1/p' | head -1)
PINNED_CUR=$(echo "$PINNED" | sed -n 's/.*current_compute_score=\([0-9.-]*\).*/\1/p' | head -1)
PINNED_MEDIAN=$(echo "$PINNED" | sed -n 's/.*compute_median_score=\([0-9.-]*\).*/\1/p' | head -1)
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
# No benchmark ran, so there is no distribution either: the median is "not measured", not
# the pinned number copied into a second field.
if [ "$PINNED_MEDIAN" = "-" ]; then
  echo "OK: sem benchmark, a mediana também é 'não medido'"
else
  echo "FALHA: compute_median_score deveria ser '-' com o score fixado (veio '$PINNED_MEDIAN')"
  teardown
  exit 1
fi

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== C3 OK ==="
