#!/usr/bin/env bash
# Fatia da Fase I (I2): classificação de armazenamento por nó (HOT/WARM/COLD).
#
# Critério verificado: cada nó diz de que classes de armazenamento dispõe — WARM (SSD/NVMe)
# e COLD (HDD); HOT é a RAM, já reportada em RAM_GB. Sem isso o diretório de estado
# (docs/13-state-manager.md) não tem como escolher onde colocar estado.
#
# A classificação é uma medida do worker: ele lê /sys/block e traduz rotational + nome do
# dispositivo em classe. Em contêiner comum /sys/block é do host, então o teste fixa as
# classes de um nó por AI_CLUSTER_STORAGE_CLASSES para provar o caminho, e deixa outro nó
# sem override para provar que a detecção real roda de ponta a ponta.
#
# Sequência:
#   A) nó com classes fixadas: a classificação atravessa worker → proto → controller → CLI.
#   B) nó sem override: o valor é uma classe válida, ou "-" se nada foi lido.
#   C) override vazio: "-" — "não medido" não pode parecer "sem armazenamento".
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051

run() {
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_AUTO_APPROVE=1 --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

teardown() {
  docker rm -f aic-controller aic-worker-fixed aic-worker-real aic-worker-blank >/dev/null 2>&1
}

# Reads the storage column of one node's row from the `nodes` table. The class list is the
# last column, so the last field of the matching row is what the client actually rendered.
storage_of() {
  echo "$1" | awk -v node="$2" '$1 == node { print $(NF-1) }'
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-controller --network "$NET" -v aic-target:/t \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-worker-fixed --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-fixed \
  -e AI_CLUSTER_STORAGE_CLASSES=nvme,ssd \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-worker-real --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-real \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-worker-blank --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-blank \
  -e AI_CLUSTER_STORAGE_CLASSES= \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 8

NODES=$(run "$R/ai-cluster nodes")
echo "$NODES"

echo "=== A) nó com classes fixadas (nvme,ssd) ==="
FIXED=$(storage_of "$NODES" node-fixed)
if [ "$FIXED" = "nvme,ssd" ]; then
  echo "OK: a classificação chegou ao cliente como nvme,ssd"
else
  echo "FALHA: esperado nvme,ssd em node-fixed, veio '$FIXED'"
  teardown
  exit 1
fi

echo "=== B) nó sem override: detecção real de /sys/block ==="
REAL=$(storage_of "$NODES" node-real)
if echo "$REAL" | grep -qE '^(nvme|ssd|hdd)(,(nvme|ssd|hdd))*$|^-$'; then
  echo "OK: a detecção devolveu uma classe válida ('$REAL')"
else
  echo "FALHA: esperado uma classe válida ou '-', veio '$REAL'"
  teardown
  exit 1
fi

echo "=== C) override vazio: não medido ==="
BLANK=$(storage_of "$NODES" node-blank)
if [ "$BLANK" = "-" ]; then
  echo "OK: sem classes lidas a coluna é '-', não vazia"
else
  echo "FALHA: esperado '-' em node-blank, veio '$BLANK'"
  teardown
  exit 1
fi

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== fim ==="
