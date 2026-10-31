#!/usr/bin/env bash
# Fatia da Fase C4: a degradação REAL de um link muda a decisão de placement.
#
# Critério verificado: um worker cujo link é estrangulado depois de entrar (tc tbf no egress do
# contêiner) passa a reportar `rede_atual` abaixo do baseline, e o escalonador manda a unidade
# para o worker de link intacto. Até aqui a degradação só era coberta por teste unitário.
#
# Os dois workers reportam a mesma capability (threads, memória, compute e saúde de storage
# fixados); o worker degradado é o primeiro (node-a), para a unidade ir ao outro só se o custo
# de transferência decidir e não a ordem de chegada. O estrangulamento só começa depois do
# registro, então o baseline de node-a é limpo e a queda aparece só na sonda ociosa.
#
# Sequência:
#   A) antes do tc, a sonda ociosa já preencheu rede_atual nos dois workers;
#   B) depois do tc, rede_atual de node-a cai bem abaixo da de node-b;
#   C) o placement manda a unidade para node-b.
set -uo pipefail

NET=aic-net-c6
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller-c6:50051

run() {
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

teardown() {
  docker rm -f aic-controller-c6 aic-worker-a aic-worker-b >/dev/null 2>&1
}

# rede_atual=<n> from `node show`, or "-" when not yet remeasured.
current() {
  run "$R/aic cluster node show $1" | grep -o 'rede_atual=[^ ]*' | cut -d= -f2
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-controller-c6 --network "$NET" -v aic-target:/t \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null

start_worker() {
  local name=$1 node=$2
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --cap-add NET_ADMIN --name "$name" --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID="$node" \
    -e AI_CLUSTER_CPU_THREADS=2 -e AI_CLUSTER_MEMORY_BYTES=1073741824 \
    -e AI_CLUSTER_COMPUTE_SCORE=1.0 -e AI_CLUSTER_STORAGE_HEALTH=1.0 \
    -e AI_CLUSTER_RECOMPUTE_INTERVAL_SECS=2 \
    "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
}

start_worker aic-worker-a node-a
start_worker aic-worker-b node-b
sleep 12

echo "=== A) antes do tc, a sonda ociosa já preencheu rede_atual nos dois workers ==="
A0=$(current node-a); B0=$(current node-b)
echo "rede_atual node-a=$A0 node-b=$B0"
if [ "$A0" = "-" ] || [ "$B0" = "-" ]; then
  echo "FALHA: a sonda ociosa ainda não preencheu rede_atual"
  teardown
  exit 1
fi

# Throttle node-a's egress to ~20 Mbit/s (the probe uploads, so it is egress that matters).
docker exec aic-worker-a sh -c 'apt-get update -qq >/dev/null 2>&1; apt-get install -y -qq iproute2 >/dev/null 2>&1; tc qdisc add dev eth0 root tbf rate 20mbit burst 32kbit latency 400ms' \
  || { echo "FALHA: não consegui aplicar tc em node-a"; teardown; exit 1; }
sleep 15

# F09 E5: o worker também mede a latência até o controller e a reporta na saúde.
LAT=$(run "$R/aic cluster node show node-b" | grep -o 'latencia_ms=[^ ]*' | cut -d= -f2)
if [ -n "$LAT" ] && [ "$LAT" != "-" ]; then
  echo "OK: latencia_ms de node-b = $LAT"
else
  echo "FALHA: node-b deveria reportar latencia_ms, veio '$LAT'"
  teardown
  exit 1
fi

echo "=== B) depois do tc, rede_atual de node-a cai bem abaixo da de node-b ==="
A1=$(current node-a); B1=$(current node-b)
echo "rede_atual node-a=$A1 node-b=$B1"
if awk -v a="$A1" -v b="$B1" 'BEGIN { exit !(a != "-" && b != "-" && a * 4 < b) }'; then
  echo "OK: node-a=$A1 < 1/4 de node-b=$B1"
else
  echo "FALHA: esperado rede_atual(node-a) < 1/4 de node-b"
  teardown
  exit 1
fi

echo "=== C) o placement manda a unidade para o worker de link intacto ==="
BENCH=$(run "$R/aic cluster benchmark --size 64 --repetitions 1")
echo "$BENCH"
RAN=$(echo "$BENCH" | awk '$1 ~ /^w-/ { print $2; exit }')
if [ "$RAN" = "node-b" ]; then
  echo "OK: a unidade rodou em node-b, o worker de link intacto"
else
  echo "FALHA: esperado node-b, veio '$RAN'"
  teardown
  exit 1
fi

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== fim ==="
