#!/usr/bin/env bash
# Fatia da Fase G (G1): filtro de candidatos por memória — o footprint da unidade decide.
#
# O critério verificado: uma unidade só é despachada para um nó que reporte memória
# suficiente para segurá-la. O footprint é derivado da própria unidade (A e B lidos,
# C escrito, 8 bytes por elemento), então nenhuma unidade precisa declarar o requisito.
#
# Sequência: subir um nó com pouca memória sozinho, enfileirar uma unidade maior que
# ele, mostrar que a unidade fica inacabada, então adicionar um nó com memória
# suficiente e mostrar que a unidade vai para ele. AI_CLUSTER_MEMORY_BYTES força a
# capability do nó para baixo, do mesmo modo que AI_CLUSTER_CPU_THREADS faz com threads,
# para que um único host represente duas máquinas de tamanhos diferentes.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051
SIZE=${SIZE:-256}
SMALL_MEM=${SMALL_MEM:-1000000}
LARGE_MEM=${LARGE_MEM:-2000000000}

run() {
  MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

echo "=== limpeza ==="
docker rm -f aic-controller aic-worker-small aic-worker-large >/dev/null 2>&1
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

# 256x256 -> (65536 + 65536 + 65536) elementos * 8 bytes = 1572864 bytes.
FOOTPRINT=$((SIZE * SIZE * 3 * 8))
echo "=== unidade ${SIZE}x${SIZE} precisa de ${FOOTPRINT} bytes ==="

echo "=== iniciar controller + apenas o nó de memória pequena (${SMALL_MEM} bytes) ==="
MSYS_NO_PATHCONV=1 docker run -d --name aic-controller --network "$NET" \
  -v aic-target:/t "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
MSYS_NO_PATHCONV=1 docker run -d --name aic-worker-small --network "$NET" \
  -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-small \
  -e AI_CLUSTER_MEMORY_BYTES="$SMALL_MEM" \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 8

echo "--- nós: o nó pequeno reporta ${SMALL_MEM} bytes ---"
run "$R/ai-cluster nodes"

echo "=== enfileirar a unidade; apenas o nó pequeno está presente e ela não cabe nele ==="
MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" \
  sh -c "$R/ai-cluster benchmark --size $SIZE --repetitions 1" &
CLI=$!
sleep 12

echo "--- log do controller durante a janela só-pequeno ---"
if docker logs aic-controller 2>&1 | grep -q "finalizado em"; then
  echo "FALHA: uma unidade finalizou num nó sem memória para segurá-la"
else
  echo "OK: nada finalizou; o nó pequeno não pegou uma unidade que não cabe nele"
fi

echo "=== iniciar o nó de memória grande (${LARGE_MEM} bytes); a unidade em espera vai para ele ==="
MSYS_NO_PATHCONV=1 docker run -d --name aic-worker-large --network "$NET" \
  -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-large \
  -e AI_CLUSTER_MEMORY_BYTES="$LARGE_MEM" \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
wait "$CLI"

echo "--- log do controller ---"
docker logs aic-controller 2>&1 | tail -12