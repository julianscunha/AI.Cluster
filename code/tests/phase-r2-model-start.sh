#!/usr/bin/env bash
# O fluxo do produto (ADR-022): o operador põe um GGUF na pasta do controller e roda `model start`.
# O controller confere a RAM, escolhe os workers, gera a chave de API (só o hash fica com ele); o
# gateway carrega o modelo pelo llama.cpp, dividido com os workers por RPC sobre mTLS. Um worker
# novo entra no plano sozinho.
#
# Precisa dos binários de release no volume aic-target e de /t/llama/{llama-server,ggml-rpc-server,
# smollm2-360m-q4.gguf} (veja code/third_party/llama.cpp/README.md).
#
# Sequência:
#   A) `model list` e `model check` enxergam o GGUF; sem worker com RPC o start diz isso.
#   B) `model start` mostra a chave; o status vai a PRONTO; o chat responde com a chave.
#   C) um segundo worker entra: depois da janela de estabilidade o plano ganha o worker, a geração
#      sobe e o modelo volta a PRONTO.
#   D) chave errada = 401; o status não mostra a chave, só o prefixo.
#   E) `model stop`: o modelo sai da API; a mesma chave volta a valer no próximo `start`.
set -uo pipefail
export MSYS_NO_PATHCONV=1

NET=aic-net
IMG=aic-rust:1
R=/t/release
L=/t/llama
JOIN=r2-join
OPERATOR=r2-operator
INFERENCE=r2-inference
PKI=$(mktemp -d -p /d/temp 2>/dev/null || mktemp -d)
PKI_MOUNT=$(cygpath -m "$PKI" 2>/dev/null || echo "$PKI")

teardown() {
  docker rm -f aic-controller aic-worker-r aic-worker-r2 aic-gateway-r >/dev/null 2>&1
}

fail() {
  echo "FALHA: $1"
  echo "--- controller"; docker logs --tail 8 aic-controller 2>&1 | cut -c1-200
  echo "--- gateway"; docker logs --tail 12 aic-gateway-r 2>&1 | cut -c1-200
  teardown
  exit 1
}

cli() {
  docker run -e AI_CLUSTER_INSECURE=1 --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_OPERATOR_TOKEN=$OPERATOR -e AI_CLUSTER_CONTROLLER=http://aic-controller:50051 \
    "$IMG" sh -c "$R/ai-cluster $*" 2>&1
}

net_curl() {
  docker run --rm --network "$NET" "$IMG" curl -s -m "${CURL_MAX:-120}" "$@"
}

worker() { # nome  id
  docker run -e AI_CLUSTER_INSECURE=1 -d --name "$1" --network "$NET" -v aic-target:/t -v "$PKI_MOUNT:/pki:ro" \
    -e AI_CLUSTER_JOIN_TOKEN=$JOIN -e AI_CLUSTER_CONTROLLER=http://aic-controller:50051 -e AI_CLUSTER_NODE_ID="$2" \
    -e AI_CLUSTER_RPC_SERVER_BIN=$L/ggml-rpc-server -e AI_CLUSTER_RPC_LISTEN=0.0.0.0:50061 \
    -e AI_CLUSTER_RPC_TLS_CERT=/pki/worker.crt -e AI_CLUSTER_RPC_TLS_KEY=/pki/worker.key \
    -e AI_CLUSTER_RPC_TLS_CLIENT_CA=/pki/ca.crt \
    "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
}

wait_status() { # trecho esperado  tentativas
  local out=
  for _ in $(seq 1 "$2"); do
    out=$(cli model status)
    echo "$out" | grep -q "$1" && { echo "$out"; return 0; }
    sleep 4
  done
  echo "$out"
  return 1
}

# --- PKI de teste ---
cd "$PKI" || exit 1
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout ca.key -out ca.crt \
  -subj "/CN=aic-test-ca" -days 2 -addext "basicConstraints=critical,CA:TRUE" \
  -addext "keyUsage=critical,keyCertSign,cRLSign" >/dev/null 2>&1
issue() {
  openssl req -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout "$1.key" -out "$1.csr" \
    -subj "/CN=$1" >/dev/null 2>&1
  printf 'subjectAltName=DNS:%s\nbasicConstraints=CA:FALSE\n' "$2" > "$1.ext"
  openssl x509 -req -in "$1.csr" -CA ca.crt -CAkey ca.key -CAcreateserial -out "$1.crt" -days 2 \
    -extfile "$1.ext" >/dev/null 2>&1
}
issue worker aic-worker
issue gateway aic-gateway
chmod 644 *.key
cd - >/dev/null

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

docker run -e AI_CLUSTER_INSECURE=1 -d --name aic-controller --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_JOIN_TOKEN=$JOIN -e AI_CLUSTER_OPERATOR_TOKEN=$OPERATOR -e AI_CLUSTER_INFERENCE_TOKEN=$INFERENCE \
  -e AI_CLUSTER_AUTO_APPROVE=1 -e AI_CLUSTER_MODELS_DIR=$L -e AI_CLUSTER_PLAN_STABLE_MS=8000 \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null

docker run -e AI_CLUSTER_INSECURE=1 -d --name aic-gateway-r --network "$NET" -v aic-target:/t -v "$PKI_MOUNT:/pki:ro" \
  -e AI_CLUSTER_INFERENCE_TOKEN=$INFERENCE -e AI_CLUSTER_CONTROLLER=http://aic-controller:50051 \
  -e AI_CLUSTER_API_KEY=chave-do-ambiente -e AI_CLUSTER_GATEWAY_ADDR=0.0.0.0:8080 \
  -e AI_CLUSTER_MODELS_DIR=$L -e AI_CLUSTER_LLAMA_SERVER_BIN=$L/llama-server \
  -e AI_CLUSTER_LLAMA_TLS_CA=/pki/ca.crt -e AI_CLUSTER_LLAMA_TLS_CERT=/pki/gateway.crt \
  -e AI_CLUSTER_LLAMA_TLS_KEY=/pki/gateway.key -e AI_CLUSTER_LLAMA_CTX=1024 \
  "$IMG" sh -c "exec $R/ai-cluster-gateway" >/dev/null
sleep 5

echo "=== A) catálogo e start sem worker com RPC ==="
LIST=$(cli model list); echo "$LIST"
echo "$LIST" | grep -E "^smollm2-360m-q4.gguf " | grep -q " sim " || fail "o GGUF deveria constar como suportado"
OUT=$(cli model check smollm2-360m-q4.gguf); echo "$OUT"
# Sem nenhum worker o start cai para o modo só-gateway e avisa.
echo "OK: catálogo"

worker aic-worker-r r-worker
sleep 8

echo "=== B) model start, chave, estado e chat ==="
START=$(cli model start smollm2-360m-q4.gguf --ctx 1024); echo "$START"
echo "$START" | grep -q "^OK" || fail "o start deveria ser aceito"
KEY=$(echo "$START" | grep -o "aic_[A-Za-z0-9_-]*" | head -1)
[ -n "$KEY" ] || fail "o start deveria mostrar a chave de API"
echo "$START" | grep -q "1 workers" || echo "(aviso: o veredito não citou 1 worker)"
STATUS=$(wait_status "PRONTO" 90) || fail "o modelo deveria ficar PRONTO"
echo "$STATUS" | grep -q "worker r-worker" || fail "o plano deveria ter o r-worker"
echo "$STATUS" | grep -q "$KEY" && fail "o status não pode mostrar a chave inteira"
GEN1=$(echo "$STATUS" | grep -o "geração [0-9]*" | head -1)
BODY='{"model":"smollm2-360m-q4","messages":[{"role":"user","content":"The capital of France is"}],"max_tokens":8,"temperature":0}'
OUT=$(net_curl -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" -d "$BODY" http://aic-gateway-r:8080/v1/chat/completions)
echo "$OUT" | cut -c1-200
echo "$OUT" | grep -q '"content"' || fail "o chat com a chave do controller deveria responder"
echo "OK: start, chave e chat ($GEN1)"

echo "=== C) worker novo entra no plano ==="
worker aic-worker-r2 r-worker2
STATUS=$(wait_status "worker r-worker2" 60) || fail "o plano deveria ganhar o r-worker2"
GEN2=$(echo "$STATUS" | grep -o "geração [0-9]*" | head -1)
[ "$GEN1" != "$GEN2" ] || fail "a geração deveria subir ($GEN1 -> $GEN2)"
STATUS=$(wait_status "PRONTO" 90) || fail "o modelo deveria voltar a PRONTO com os dois workers"
OUT=$(net_curl -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" -d "$BODY" http://aic-gateway-r:8080/v1/chat/completions)
echo "$OUT" | grep -q '"content"' || fail "o chat deveria responder com os dois workers"
echo "OK: plano com dois workers ($GEN2)"

echo "=== D) autenticação ==="
CODE=$(net_curl -o /dev/null -w "%{http_code}" -H "Authorization: Bearer errada" -H "Content-Type: application/json" -d "$BODY" http://aic-gateway-r:8080/v1/chat/completions)
[ "$CODE" = 401 ] || fail "chave errada deveria dar 401 (deu $CODE)"
echo "OK: 401"

echo "=== E) model stop e novo start com a mesma chave ==="
cli model stop | grep -q "parado" || fail "o stop deveria confirmar"
for _ in $(seq 1 20); do
  CODE=$(net_curl -o /dev/null -w "%{http_code}" -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" -d "$BODY" http://aic-gateway-r:8080/v1/chat/completions)
  [ "$CODE" = 404 ] && break
  sleep 3
done
[ "$CODE" = 404 ] || fail "parado, o modelo deveria sair da API (deu $CODE)"
START=$(cli model start smollm2-360m-q4.gguf --ctx 1024); echo "$START" | head -2
echo "$START" | grep -q "CHAVE DE API" && fail "a chave já existe: o start não deveria criar outra"
wait_status "PRONTO" 90 >/dev/null || fail "o modelo deveria voltar a PRONTO"
OUT=$(net_curl -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" -d "$BODY" http://aic-gateway-r:8080/v1/chat/completions)
echo "$OUT" | grep -q '"content"' || fail "a mesma chave deveria continuar valendo"
echo "OK: stop e start com a mesma chave"

teardown
rm -rf "$PKI"
echo "=== fluxo models start OK ==="
