#!/usr/bin/env bash
# Fatia da Fase 2: placement — o requisito de uma unidade decide qual nó a executa.
#
# O critério de aceitação verificado: o scheduler escolhe placement melhor do que
# primeiro-a-chegar. Uma unidade que declara min_cpu_threads é recusada a um
# nó que reporta menos, então espera por um nó capaz em vez de ser entregue ao
# primeiro que faz poll. Sem o filtro, o nó fraco a pega e a execução
# termina com a unidade em node-weak.
#
# Sequência: subir um nó fraco sozinho, enfileirar uma unidade que precisa de mais threads do que
# ele tem, mostrar que a unidade fica inacabada, então adicionar um nó forte e mostrar que a unidade
# vai para ele. A capability do nó fraco é forçada para baixo com AI_CLUSTER_CPU_THREADS
# para que um único host represente duas máquinas de tamanhos diferentes.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051
SIZE=${SIZE:-256}
WEAK_THREADS=${WEAK_THREADS:-2}
STRONG_THREADS=${STRONG_THREADS:-8}
MIN_THREADS=${MIN_THREADS:-4}

run() {
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

echo "=== limpeza ==="
docker rm -f aic-controller aic-worker-weak aic-worker-strong >/dev/null 2>&1
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

echo "=== iniciar controller + apenas o nó fraco ==="
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-controller --network "$NET" \
  -v aic-target:/t "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-worker-weak --network "$NET" \
  -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-weak \
  -e AI_CLUSTER_CPU_THREADS="$WEAK_THREADS" \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 8

echo "--- nós: o nó fraco reporta $WEAK_THREADS threads ---"
run "$R/aic cluster nodes"

echo "=== enfileirar uma unidade que precisa de $MIN_THREADS threads; apenas o nó fraco está presente ==="
# O cliente faz poll até a fila drenar, o que não pode acontecer enquanto o único nó
# presente for fraco demais para pegar a unidade. Então roda em background e espera-se
# que ainda esteja fazendo poll quando o nó forte for adicionado abaixo.
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 --rm --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" \
  sh -c "$R/aic cluster benchmark --size $SIZE --repetitions 1 --min-threads $MIN_THREADS" &
CLI=$!
sleep 12

echo "--- log do controller durante a janela só-fraco ---"
if docker logs aic-controller 2>&1 | grep -q "finalizado em"; then
  echo "FALHA: uma unidade finalizou enquanto apenas o nó fraco estava presente"
else
  echo "OK: nada finalizou; o nó fraco não pegou a unidade pesada"
fi

echo "=== iniciar o nó forte; a unidade em espera deve ir para ele ==="
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-worker-strong --network "$NET" \
  -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-strong \
  -e AI_CLUSTER_CPU_THREADS="$STRONG_THREADS" \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
wait "$CLI"

echo "--- log do controller ---"
docker logs aic-controller 2>&1 | tail -12
