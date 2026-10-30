#!/usr/bin/env bash
# Fatia da Fase G (G6): reservas — um nó fica retido para uma requisição futura.
#
# Critério verificado: a reserva decide o placement, e não o custo. Um nó reservado
# (a) recusa unidades de outras requisições e (b) recebe as unidades da requisição dona
# mesmo sendo o nó pior, o que o cost model sozinho nunca faria.
#
# Dois nós, um fraco (2 threads) e um forte (8 threads). Sem reserva o cost model sempre
# prefere o forte; é essa preferência que dá sentido às pernas abaixo.
#   A) baseline: sem reserva a unidade roda no nó forte.
#   B) dono recebe o nó: node-weak retido para req-held; a unidade de req-held roda no
#      node-weak — o nó *pior* — porque a reserva vence o ranking de custo.
#   C) recusa: node-strong retido para req-locked; a unidade de req-other não pode usar o
#      node-strong e cai no node-weak, mesmo o forte estando ocioso.
#   D) liberar: soltar a reserva devolve o nó forte ao ranking.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051
SIZE=${SIZE:-256}
WEAK_THREADS=${WEAK_THREADS:-2}
STRONG_THREADS=${STRONG_THREADS:-8}
MINUTES=${MINUTES:-1}

run() {
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

teardown() {
  docker rm -f aic-controller aic-worker-weak aic-worker-strong >/dev/null 2>&1
}

# Enfileira uma unidade sob um id de requisição e imprime o nó que a executou.
#
# O relatório do benchmark é cumulativo (lista toda unidade já concluída no controller),
# então pegamos o id de trabalho mais alto — que é sempre a unidade desta perna — em vez
# de qualquer linha, que misturaria o resultado das pernas anteriores.
place() {
  run "$R/aic cluster benchmark --size $SIZE --repetitions 1 --request-id $1" \
    | awk '/^w-/{split($1, parts, "-"); if (parts[2] + 0 >= top) { top = parts[2] + 0; node = $2 }} END{print node}'
}

expect() {  # expect <rótulo> <esperado> <obtido>
  if [ "$3" = "$2" ]; then
    echo "OK: $1 rodou em $2"
  else
    echo "FALHA: $1 deveria rodar em $2, rodou em '${3:-<nada>}'"
    exit 1
  fi
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-controller --network "$NET" -v aic-target:/t \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-worker-weak --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-weak \
  -e AI_CLUSTER_CPU_THREADS="$WEAK_THREADS" \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-worker-strong --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-strong \
  -e AI_CLUSTER_CPU_THREADS="$STRONG_THREADS" \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 8

echo "=== A) baseline: sem reserva o cost model prefere o nó forte ==="
expect "baseline" node-strong "$(place req-base)"

echo "=== B) o dono recebe o nó: reserva em node-weak ($WEAK_THREADS threads) para req-held ==="
run "$R/aic cluster reservation add node-weak req-held --minutes $MINUTES"
run "$R/aic cluster reservation list"
expect "unidade de req-held" node-weak "$(place req-held)"
run "$R/aic cluster reservation remove node-weak"

echo "=== C) recusa: reserva em node-strong ($STRONG_THREADS threads) para req-locked ==="
run "$R/aic cluster reservation add node-strong req-locked --minutes $MINUTES"
expect "unidade de req-other" node-weak "$(place req-other)"

echo "=== D) liberar devolve o nó forte ao ranking ==="
run "$R/aic cluster reservation remove node-strong"
expect "unidade de req-free" node-strong "$(place req-free)"

echo "=== E) nenhuma reserva ativa ao final ==="
LISTED=$(run "$R/aic cluster reservation list")
echo "$LISTED"
grep -q '^RESERVAS 0$' <<<"$LISTED" \
  || { echo "FALHA: ainda há reserva ativa"; exit 1; }
echo "OK: nenhuma reserva ativa"

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== G6 OK ==="
