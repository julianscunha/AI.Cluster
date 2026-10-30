#!/usr/bin/env bash
# Fatia da Fase G (G3): scheduler global — o custo decide o placement, não a ordem de chegada.
#
# Critério verificado: o scheduler escolhe placement melhor que primeiro-a-chegar.
# Uma unidade que os dois nós *podem* rodar é retida do nó fraco quando um nó capaz
# está ocioso, então roda no nó capaz em vez de ir para o primeiro que faz poll.
#
# Sequência, com a mesma unidade de tamanho fixo:
#   A) baseline primeiro-a-chegar: só o nó fraco está presente, então a unidade roda nele.
#   B) placement: nó fraco e forte presentes e ociosos, então o fraco cede e a unidade
#      roda no forte — mesmo podendo rodar no fraco.
# A leg B é determinística: o fraco sempre cede enquanto o forte estiver ocioso, então a
# unidade vai para o forte independente de quem faz poll primeiro. A leg A mostra o que
# o primeiro-a-chegar faz sem essa decisão.
#
# Os dois nós rodam no mesmo host, então compartilham a CPU e o compute real seria o
# mesmo. AI_CLUSTER_WORK_DELAY_MS segura a unidade no nó fraco para representar uma
# máquina mais lenta: é o ganho que o placement captura ao rotear para o nó capaz.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051
SIZE=${SIZE:-256}
WEAK_THREADS=${WEAK_THREADS:-2}
STRONG_THREADS=${STRONG_THREADS:-8}
WEAK_DELAY_MS=${WEAK_DELAY_MS:-2000}

run() {
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

teardown() {
  docker rm -f aic-controller aic-worker-weak aic-worker-strong >/dev/null 2>&1
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

echo "=== A) baseline primeiro-a-chegar: apenas o nó fraco ($WEAK_THREADS threads, ${WEAK_DELAY_MS}ms de atraso) ==="
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-controller --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_PLACEMENT=off "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-worker-weak --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-weak \
  -e AI_CLUSTER_CPU_THREADS="$WEAK_THREADS" -e AI_CLUSTER_WORK_DELAY_MS="$WEAK_DELAY_MS" \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 8

BASE=$(run "$R/aic cluster benchmark --size $SIZE --repetitions 1" | grep '^w-' || true)
echo "$BASE"
if grep -q 'node-weak' <<<"$BASE"; then
  echo "OK: primeiro-a-chegar entregou a unidade ao nó fraco"
else
  echo "FALHA: o baseline deveria mostrar node-weak"
  exit 1
fi

teardown
echo "=== B) placement: nó fraco + nó forte ($STRONG_THREADS threads), ambos ociosos ==="
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-controller --network "$NET" -v aic-target:/t \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
for name in weak strong; do
  if [ "$name" = weak ]; then
    threads=$WEAK_THREADS
    delay="-e AI_CLUSTER_WORK_DELAY_MS=$WEAK_DELAY_MS"
  else
    threads=$STRONG_THREADS
    delay=""
  fi
  # shellcheck disable=SC2086
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name "aic-worker-$name" --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID="node-$name" \
    -e AI_CLUSTER_CPU_THREADS="$threads" $delay \
    "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
done
sleep 8

echo "--- nós presentes ---"
run "$R/aic cluster nodes"

PLACED=$(run "$R/aic cluster benchmark --size $SIZE --repetitions 1" | grep '^w-' || true)
echo "$PLACED"
if grep -q 'node-strong' <<<"$PLACED" && ! grep -q 'node-weak' <<<"$PLACED"; then
  echo "OK: placement entregou a unidade ao nó forte, que estava ocioso"
else
  echo "FALHA: o placement deveria entregar a unidade a node-strong, sem node-weak"
  exit 1
fi

echo "=== comparação: mesma unidade, nó que a executou e compute ==="
echo "primeiro-a-chegar: $BASE"
echo "placement:         $PLACED"

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== fim ==="