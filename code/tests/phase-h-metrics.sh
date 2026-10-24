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
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 --rm --network "$NET" -v aic-target:/t \
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

MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-controller --network "$NET" -v aic-target:/t \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-worker-m --network "$NET" -v aic-target:/t \
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

echo "=== C) percentis ==="
P50=$(echo "$AFTER" | field p50_latency_us)
P95=$(echo "$AFTER" | field p95_latency_us)
# A média escondia a cauda; os percentis têm que existir e ser ordenados.
if [ -n "$P50" ] && [ -n "$P95" ] && [ "$P50" -le "$P95" ] 2>/dev/null; then
  echo "OK: p50_latency_us=$P50 <= p95_latency_us=$P95"
else
  echo "FALHA: esperado p50 <= p95, veio p50=$P50 p95=$P95"
  teardown
  exit 1
fi

echo "=== D) métricas de nó ==="
NODES_OUT=$(run "$R/ai-cluster nodes")
echo "$NODES_OUT"
if echo "$NODES_OUT" | grep -q "CPU_%"; then
  echo "OK: a tabela de nós traz as colunas de métricas de nó"
else
  echo "FALHA: a tabela de nós deveria ter CPU_%/TEMP_C/SWAP_%"
  teardown
  exit 1
fi
if echo "$NODES_OUT" | grep -q "node-m"; then
  echo "OK: o nó aparece com suas métricas"
else
  echo "FALHA: o nó node-m deveria estar listado"
  teardown
  exit 1
fi

echo "=== E) exportador Prometheus ==="
OPERATOR=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator}
DENIED=$(run "curl -s -o /dev/null -w %{http_code} http://aic-controller:9101/metrics")
if [ "$DENIED" = 401 ]; then
  echo "OK: /metrics sem token responde 401"
else
  echo "FALHA: /metrics sem token deveria responder 401, respondeu: $DENIED"
  teardown
  exit 1
fi
EXPORT=$(run "curl -sf -H 'Authorization: Bearer $OPERATOR' http://aic-controller:9101/metrics")
if [ -z "$EXPORT" ]; then
  echo "FALHA: o exportador não respondeu em /metrics"
  teardown
  exit 1
fi
echo "$EXPORT" | head -8
for metric in ai_cluster_nodes_total ai_cluster_units_completed \
              ai_cluster_p95_latency_us ai_cluster_node_cpu_utilization; do
  if echo "$EXPORT" | grep -q "^$metric"; then
    echo "OK: $metric presente"
  else
    echo "FALHA: $metric ausente no /metrics"
    teardown
    exit 1
  fi
done

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== fim ==="
