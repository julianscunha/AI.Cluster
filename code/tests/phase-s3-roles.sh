#!/usr/bin/env bash
# Fatia da Fase S (S3a): negação de papel.
#
# Critério verificado: quem tem só o token de join, ou só o de inferência, não administra o
# cluster. A CLI manda o segredo errado como se fosse o de operador e recebe PermissionDenied
# nas RPCs administrativas; o segredo certo passa. Leituras aceitam join e operador.
#
# Sequência:
#   A) token de operador certo: `approve` responde, `nodes` lista;
#   B) token de join no lugar do de operador: `approve` e o enfileiramento são negados;
#   C) token de inferência no lugar do de operador: nada passa, nem leitura;
#   D) segredo inventado: Unauthenticated.
set -uo pipefail

NET=aic-net-s3
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller-s3:50051
JOIN=s3-join
OPERATOR=s3-operator
INFERENCE=s3-inference

teardown() { docker rm -f aic-controller-s3 aic-worker-s3 >/dev/null 2>&1; }
fail() { echo "FALHA: $1"; teardown; exit 1; }

# cli <segredo-de-operador> <args...>
cli() {
  local token=$1
  shift
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 --rm --network "$NET" -v aic-target:/t \
    -e "AI_CLUSTER_OPERATOR_TOKEN=$token" -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$R/ai-cluster $*" 2>&1
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -d --name aic-controller-s3 --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_JOIN_TOKEN=$JOIN -e AI_CLUSTER_OPERATOR_TOKEN=$OPERATOR -e AI_CLUSTER_INFERENCE_TOKEN=$INFERENCE \
  -e AI_CLUSTER_AUTO_APPROVE=1 "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -d --name aic-worker-s3 --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_JOIN_TOKEN=$JOIN -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-s3 \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 6

echo "=== A) o token de operador administra ==="
cli $OPERATOR nodes | grep -q node-s3 || fail "o operador deveria listar o nó"
cli $OPERATOR approve node-s3 | grep -qi "aprovad" || fail "o operador deveria aprovar o nó"
echo "OK"

echo "=== B) o token de join não administra, mas lê ==="
cli $JOIN nodes | grep -q node-s3 || fail "o token de join deveria poder listar"
OUT=$(cli $JOIN approve node-s3); echo "$OUT"
echo "$OUT" | grep -qi "PermissionDenied\|permission" || fail "approve com token de join deveria ser negado"
OUT=$(cli $JOIN benchmark --size 8 --repetitions 1); echo "$OUT"
echo "$OUT" | grep -qi "PermissionDenied\|permission" || fail "enfileirar com token de join deveria ser negado"
OUT=$(cli $JOIN revoke node-s3); echo "$OUT"
echo "$OUT" | grep -qi "PermissionDenied\|permission" || fail "revoke com token de join deveria ser negado"
echo "OK: approve, enfileirar e revoke negados ao token de join"

echo "=== C) o token de inferência só chama ShardMatmul ==="
for args in "nodes" "approve node-s3" "benchmark --size 8 --repetitions 1"; do
  OUT=$(cli $INFERENCE $args); echo "$args -> $OUT" | head -2
  echo "$OUT" | grep -qi "PermissionDenied\|permission" || fail "'$args' com token de inferência deveria ser negado"
done
echo "OK: token de inferência negado em leitura e administração"

echo "=== D) segredo inventado ==="
OUT=$(cli inventado nodes); echo "$OUT"
echo "$OUT" | grep -qi "Unauthenticated\|unauthenticated" || fail "segredo inventado deveria ser Unauthenticated"
echo "OK"

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== fim ==="
