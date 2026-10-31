#!/usr/bin/env bash
# Catálogo de modelos e veredito OK/NOK (ADR-022, E1 e E2).
#
# Critério verificado: o controller lê os cabeçalhos da pasta AI_CLUSTER_MODELS_DIR e `model list`
# diz o que é cada modelo e se o motor o roda; `model check` compara o modelo com a RAM dos workers
# aprovados e vivos: OK quando cabe, NOK com o motivo e quanto falta quando não cabe.
#
# Precisa dos pesos em code/models/{gpt2,smollm2} (ignorados pelo git) e dos binários de release
# no volume aic-target.
#
# Sequência:
#   A) `model list` mostra gpt2 e smollm2 como Llama/GPT-2 suportados e um GGUF inválido com motivo.
#   B) `model check` de um modelo que cabe na RAM do worker de teste: OK.
#   C) `model check` com a RAM do worker reduzida (AI_CLUSTER_MEMORY_BYTES): NOK com "faltam".
#   D) token de join não lista nem verifica modelos.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051
JOIN=m-join
OPERATOR=m-operator
MODELS=$(cd "$(dirname "$0")/.." && pwd -W 2>/dev/null || (cd "$(dirname "$0")/.." && pwd))/models

teardown() {
  docker rm -f aic-controller aic-worker >/dev/null 2>&1
}

cli() {
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_OPERATOR_TOKEN=${CLI_TOKEN:-$OPERATOR} -e AI_CLUSTER_CONTROLLER=$CONTROLLER \
    "$IMG" sh -c "$R/aic $*" 2>&1
}

fail() {
  echo "FALHA: $1"
  teardown
  exit 1
}

[ -d "$MODELS/gpt2" ] && [ -d "$MODELS/smollm2" ] || fail "faltam os pesos em code/models/{gpt2,smollm2}"

# Pasta de teste: os dois modelos reais (somente leitura) mais um GGUF quebrado.
TMP=$(mktemp -d)
printf 'not a gguf' > "$TMP/quebrado.gguf"

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

start_controller_and_worker() {
  teardown
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -d --name aic-controller --network "$NET" -v aic-target:/t \
    -v "$MODELS/gpt2:/models/gpt2:ro" -v "$MODELS/smollm2:/models/smollm2:ro" \
    -v "$(cygpath -m "$TMP" 2>/dev/null || echo "$TMP")/quebrado.gguf:/models/quebrado.gguf:ro" \
    -e AI_CLUSTER_JOIN_TOKEN=$JOIN -e AI_CLUSTER_OPERATOR_TOKEN=$OPERATOR -e AI_CLUSTER_AUTO_APPROVE=1 \
    -e AI_CLUSTER_MODELS_DIR=/models \
    "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -d --name aic-worker --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_JOIN_TOKEN=$JOIN -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID=node-m \
    ${1:+-e AI_CLUSTER_MEMORY_BYTES=$1} "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
  sleep 8
}

echo "=== A) model list ==="
start_controller_and_worker
LIST=$(cli cluster model list)
echo "$LIST"
echo "$LIST" | grep -E "^gpt2 " | grep -q " sim " || fail "gpt2 deveria constar como suportado"
echo "$LIST" | grep -E "^smollm2 " | grep -q " sim " || fail "smollm2 deveria constar como suportado"
echo "$LIST" | grep -E "^quebrado.gguf " | grep -q "inválido" || fail "o GGUF quebrado deveria constar com o motivo"
echo "OK: catálogo lido dos cabeçalhos"

echo "=== B) model check cabe ==="
OUT=$(cli cluster model check gpt2)
echo "$OUT"
echo "$OUT" | grep -q "^OK" || fail "gpt2 deveria caber na RAM do worker de teste"

echo "=== C) model check não cabe: NOK com o que falta ==="
start_controller_and_worker 134217728   # 128 MiB: abaixo da reserva por worker
OUT=$(cli cluster model check smollm2)
echo "$OUT"
echo "$OUT" | grep -q "^NOK" || fail "smollm2 não deveria caber em 128 MiB"
echo "$OUT" | grep -q "faltam" || fail "o NOK deveria dizer quanto falta"
echo "$OUT" | grep -q "acrescente" || fail "o NOK deveria dizer quantos workers acrescentar"

echo "=== D) token de join não lista modelos ==="
DENIED=$(CLI_TOKEN=$JOIN cli cluster model list)
echo "$DENIED"
echo "$DENIED" | grep -qi "operador" || fail "o token de join deveria ser recusado"

teardown
rm -rf "$TMP"
echo "=== catálogo e OK/NOK OK ==="
