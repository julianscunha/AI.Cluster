#!/usr/bin/env bash
# Fatia da Fase C (C1): descoberta de rede.
#
# Critério verificado: o nó reporta a própria rede como capability, e esse número é o que o
# modelo de placement usa para decidir onde a unidade roda. É descoberta, não benchmark: a
# velocidade *nominal* do link, relativa à referência de 1 GbE. Medir a taxa alcançada sob
# carga e a latência é a fatia de benchmark (docs/05), não esta.
#
# Sequência:
#   A) nó real: o detalhe traz `rede_score=`, a velocidade do link sobre 1 GbE. Um veth de
#      contêiner não tem taxa negociada, então `-` é a resposta honesta ali; o que o teste
#      exige é o campo presente e a convenção de zero = "não medido" respeitada.
#   B) `AI_CLUSTER_NETWORK_SCORE` fixado: o nó reporta o número pinado sem ler interface — é
#      o botão de determinismo que o passo C usa.
#   C) o consumidor: dois nós com o mesmo compute e redes diferentes. Primeiro-a-chegar manda
#      a unidade para o nó de link lento; com placement, o custo a manda para o link rápido.
#      Sem a perna do baseline não daria para dizer se a decisão foi o custo ou o acaso.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051
SIZE=64
SCORE=1.0
THREADS=4
SLOW_LINK=1.0
FAST_LINK=10.0

run() {
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

# start_controller [ENV=valor] — sem argumento, sobe com o placement ligado (o padrão).
start_controller() {
  local extra=()
  if [ -n "${1:-}" ]; then
    extra=(-e "$1")
  fi
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -d --name aic-controller --network "$NET" -v aic-target:/t \
    "${extra[@]}" "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
}

# start_worker <nome> <nó> <extra env...>
start_worker() {
  local name=$1 node=$2
  shift 2
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -d --name "$name" --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID="$node" "$@" \
    "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
}

teardown() {
  docker rm -f aic-controller aic-worker-net aic-worker-slow aic-worker-fast >/dev/null 2>&1
}

# field <chave> — lê o valor de uma linha `chave=valor` do detalhe do nó.
field() {
  sed -n "s/.*$1=\([0-9.-]*\).*/\1/p" | head -1
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

echo "=== A) nó real: a rede sai do link, não de um pin ==="
start_controller
start_worker aic-worker-net node-net -e AI_CLUSTER_RECOMPUTE_INTERVAL_SECS=0
sleep 12

DETAIL=$(run "$R/ai-cluster node show node-net")
echo "$DETAIL"
REDE=$(echo "$DETAIL" | field rede_score)
if [ -z "$REDE" ]; then
  echo "FALHA: o detalhe deveria trazer rede_score="
  teardown
  exit 1
fi
if [ "$REDE" = "-" ]; then
  echo "OK: sem taxa negociada neste contêiner, a rede é 'não medido' (-), não zero"
elif awk -v s="$REDE" 'BEGIN { exit !(s+0 > 0) }'; then
  echo "OK: a rede foi descoberta do link: rede_score=$REDE sobre a referência de 1 GbE"
else
  echo "FALHA: rede_score deveria ser '-' ou > 0, veio '$REDE'"
  teardown
  exit 1
fi

ARMAZ=$(echo "$DETAIL" | field armaz_score)
if awk -v s="$ARMAZ" 'BEGIN { exit !(s+0 > 0) }'; then
  echo "OK: o benchmark de disco mediu armaz_score=$ARMAZ"
else
  echo "FALHA: armaz_score deveria ser > 0, veio '$ARMAZ'"
  teardown
  exit 1
fi
ATUAL=$(echo "$DETAIL" | field rede_atual)
echo "rede_atual=$ATUAL"
ALCANCADA=$(echo "$DETAIL" | field rede_alcancada)
if awk -v s="$ALCANCADA" -v c="$ATUAL" 'BEGIN { exit !(s+0 > 0 && c+0 > 0) }'; then
  echo "OK: a sonda ProbeLink mediu a taxa alcançada: rede_alcancada=$ALCANCADA, remedida rede_atual=$ATUAL"
else
  echo "FALHA: rede_alcancada deveria ser > 0, veio '$ALCANCADA'"
  teardown
  exit 1
fi

teardown
echo "=== B) AI_CLUSTER_NETWORK_SCORE fixado: o pin manda no número ==="
start_controller
start_worker aic-worker-net node-net -e AI_CLUSTER_NETWORK_SCORE=4.0
sleep 8

PINNED=$(run "$R/ai-cluster node show node-net")
echo "$PINNED"
REDE=$(echo "$PINNED" | field rede_score)
ALCANCADA=$(echo "$PINNED" | field rede_alcancada)
if [ "$REDE" = "4.00" ] && [ "$ALCANCADA" = "-" ]; then
  echo "OK: o score pinado foi reportado como veio, sem ler interface e sem sondar"
else
  echo "FALHA: esperado 4.00, veio '$REDE'"
  teardown
  exit 1
fi

teardown
echo "=== C) baseline primeiro-a-chegar: só o nó de link lento ==="
start_controller AI_CLUSTER_PLACEMENT=off
start_worker aic-worker-slow node-slow \
  -e AI_CLUSTER_COMPUTE_SCORE=$SCORE -e AI_CLUSTER_CPU_THREADS=$THREADS \
  -e AI_CLUSTER_NETWORK_SCORE=$SLOW_LINK
sleep 8

BASE=$(run "$R/ai-cluster benchmark --size $SIZE --repetitions 1" | grep '^w-' || true)
echo "$BASE"
if grep -q 'node-slow' <<<"$BASE"; then
  echo "OK: primeiro-a-chegar entregou a unidade ao nó de link lento"
else
  echo "FALHA: o baseline deveria mostrar node-slow"
  teardown
  exit 1
fi

teardown
echo "=== D) placement: os dois ociosos, mesmo compute, links diferentes ==="
start_controller
start_worker aic-worker-slow node-slow \
  -e AI_CLUSTER_COMPUTE_SCORE=$SCORE -e AI_CLUSTER_CPU_THREADS=$THREADS \
  -e AI_CLUSTER_NETWORK_SCORE=$SLOW_LINK
start_worker aic-worker-fast node-fast \
  -e AI_CLUSTER_COMPUTE_SCORE=$SCORE -e AI_CLUSTER_CPU_THREADS=$THREADS \
  -e AI_CLUSTER_NETWORK_SCORE=$FAST_LINK
sleep 8

NODES=$(run "$R/ai-cluster nodes")
echo "$NODES"
ROW=$(run "$R/ai-cluster benchmark --size $SIZE --repetitions 1" | grep '^w-' || true)
echo "$ROW"
if grep -q 'node-fast' <<<"$ROW"; then
  echo "OK: o custo mandou a unidade para o link rápido ($FAST_LINK contra $SLOW_LINK)"
else
  echo "FALHA: esperado node-fast, veio '$ROW'"
  teardown
  exit 1
fi

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== C1 rede OK ==="
