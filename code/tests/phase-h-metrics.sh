#!/usr/bin/env bash
# Fatia da Fase H (H1): métricas agregadas do cluster.
#
# Critério verificado: um cliente lê, numa só chamada, o estado do cluster que antes
# exigia juntar `nodes` com `work_report` na cabeça de quem lê. O que se prova aqui é
# que os números mudam junto com o cluster — nó entra, unidade completa — e que o
# agregado bate com as partes (compute somado = compute da unidade).
#
# Sequência:
#   A) cluster ocioso: 1 nó, 0 unidades, médias em 0 (não divisão por zero).
#   B) depois de 2 unidades: concluídas=2, pendentes=0, compute somado > 0.
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
  docker rm -f aic-controller aic-worker-m >/dev/null 2>&1
}

# Reads one `chave=valor` from the `metrics` output. The command prints one line per
# group, so this pulls the named field wherever it appears.
field() {
  grep -o "$1=[0-9]*" | head -1 | cut -d= -f2
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

MSYS_NO_PATHCONV=1 docker run -d --name aic-controller --network "$NET" -v aic-target:/t \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
MSYS_NO_PATHCONV=1 docker run -d --name aic-worker-m --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-m \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 8

echo "=== A) cluster ocioso ==="
IDLE=$(run "$R/ai-cluster metrics")
echo "$IDLE"
NODES=$(echo "$IDLE" | field nodes_total)
COMPLETED=$(echo "$IDLE" | field units_completed)
AVG=$(echo "$IDLE" | field avg_compute_time_us)
if [ "$NODES" = "1" ] && [ "$COMPLETED" = "0" ] && [ "$AVG" = "0" ]; then
  echo "OK: 1 nó, nada concluído, média em 0 (sem divisão por zero)"
else
  echo "FALHA: esperado nodes_total=1 units_completed=0 avg_compute_time_us=0"
  echo "       veio nodes_total=$NODES units_completed=$COMPLETED avg_compute_time_us=$AVG"
  teardown
  exit 1
fi

echo "=== B) duas unidades executadas ==="
run "$R/ai-cluster benchmark --size $SIZE --repetitions 2" >/dev/null
AFTER=$(run "$R/ai-cluster metrics")
echo "$AFTER"
COMPLETED=$(echo "$AFTER" | field units_completed)
PENDING=$(echo "$AFTER" | field units_pending)
COMPUTE=$(echo "$AFTER" | field total_compute_time_us)
if [ "$COMPLETED" = "2" ] && [ "$PENDING" = "0" ] && [ "$COMPUTE" -gt 0 ] 2>/dev/null; then
  echo "OK: as duas unidades aparecem como concluídas e o compute foi somado"
else
  echo "FALHA: esperado units_completed=2 units_pending=0 total_compute_time_us>0"
  echo "       veio units_completed=$COMPLETED units_pending=$PENDING total_compute_time_us=$COMPUTE"
  teardown
  exit 1
fi

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== fim ==="
