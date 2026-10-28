#!/usr/bin/env bash
# Modelo GGUF servido pelo llama.cpp dividido entre gateway e worker por RPC sobre mTLS (ADR-022).
#
# Critério verificado: o gateway sobe o `llama-server`, que usa o `rpc-server` de um worker por um
# túnel mTLS; a API responde com a mesma autenticação de sempre; o `rpc-server` do worker só é
# alcançável pelo túnel (loopback + mTLS), nunca em texto puro.
#
# Precisa dos binários de release no volume aic-target e de /t/llama/{llama-server,ggml-rpc-server,
# smollm2-360m-q4.gguf} (veja code/third_party/llama.cpp/README.md).
#
# Sequência:
#   A) o worker anuncia a porta do túnel e o modelo fica pronto no gateway.
#   B) chat sem stream e com stream respondem, com `usage`.
#   C) chave errada = 401; modelo desconhecido cai no caminho normal (404).
#   D) o worker usou de fato o rpc-server (conexão aceita) e a porta local dele não é alcançável.
#   E) texto puro no túnel e cliente sem certificado da CA não chegam ao rpc-server.
set -uo pipefail
export MSYS_NO_PATHCONV=1  # o Git Bash do Windows trocaria "/CN=..." do openssl por um caminho

NET=aic-net
IMG=aic-rust:1
R=/t/release
L=/t/llama
JOIN=r-join
OPERATOR=r-operator
KEY=r-key
PKI=$(mktemp -d -p /d/temp 2>/dev/null || mktemp -d)
PKI_MOUNT=$(cygpath -m "$PKI" 2>/dev/null || echo "$PKI")

teardown() {
  docker rm -f aic-controller aic-worker-r aic-gateway-r >/dev/null 2>&1
}

fail() {
  echo "FALHA: $1"
  echo "--- gateway"; docker logs --tail 15 aic-gateway-r 2>&1 | cut -c1-200
  echo "--- worker"; docker logs --tail 10 aic-worker-r 2>&1 | cut -c1-200
  teardown
  exit 1
}

net_curl() {
  MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" "$IMG" curl -s -m "${CURL_MAX:-120}" "$@"
}

# --- PKI de teste: uma CA, o certificado do worker (servidor) e o do gateway (cliente) ---
cd "$PKI" || exit 1
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout ca.key -out ca.crt \
  -subj "/CN=aic-test-ca" -days 2 -addext "basicConstraints=critical,CA:TRUE" \
  -addext "keyUsage=critical,keyCertSign,cRLSign" >/dev/null 2>&1
issue() { # nome  SAN
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

MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -d --name aic-controller --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_JOIN_TOKEN=$JOIN -e AI_CLUSTER_OPERATOR_TOKEN=$OPERATOR -e AI_CLUSTER_AUTO_APPROVE=1 \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null

MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -d --name aic-worker-r --network "$NET" -v aic-target:/t \
  -v "$PKI_MOUNT:/pki:ro" \
  -e AI_CLUSTER_JOIN_TOKEN=$JOIN -e AI_CLUSTER_CONTROLLER=http://aic-controller:50051 -e AI_CLUSTER_NODE_ID=r-worker \
  -e AI_CLUSTER_RPC_SERVER_BIN=$L/ggml-rpc-server -e AI_CLUSTER_RPC_LISTEN=0.0.0.0:50061 \
  -e AI_CLUSTER_RPC_TLS_CERT=/pki/worker.crt -e AI_CLUSTER_RPC_TLS_KEY=/pki/worker.key \
  -e AI_CLUSTER_RPC_TLS_CLIENT_CA=/pki/ca.crt -e AI_CLUSTER_RPC_VERSION=test \
  "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
sleep 8

echo "=== A) worker anuncia o túnel e o modelo fica pronto ==="
docker logs aic-worker-r 2>&1 | grep -q "túnel RPC (mTLS) em 0.0.0.0:50061" || fail "o worker deveria abrir o túnel mTLS"
docker logs aic-worker-r 2>&1 | grep -q "rpc-server desligado" && fail "o worker não deveria desligar o rpc-server"

MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -d --name aic-gateway-r --network "$NET" -v aic-target:/t \
  -v "$PKI_MOUNT:/pki:ro" \
  -e AI_CLUSTER_OPERATOR_TOKEN=$OPERATOR -e AI_CLUSTER_CONTROLLER=http://aic-controller:50051 \
  -e AI_CLUSTER_API_KEY=$KEY -e AI_CLUSTER_GATEWAY_ADDR=0.0.0.0:8080 \
  -e AI_CLUSTER_LLAMA_MODEL=$L/smollm2-360m-q4.gguf -e AI_CLUSTER_LLAMA_MODEL_ID=smollm2-q4 \
  -e AI_CLUSTER_LLAMA_SERVER_BIN=$L/llama-server -e AI_CLUSTER_LLAMA_RPC_ENDPOINTS=aic-worker-r:50061 \
  -e AI_CLUSTER_LLAMA_TLS_CA=/pki/ca.crt -e AI_CLUSTER_LLAMA_TLS_CERT=/pki/gateway.crt \
  -e AI_CLUSTER_LLAMA_TLS_KEY=/pki/gateway.key -e AI_CLUSTER_LLAMA_CTX=1024 \
  "$IMG" sh -c "exec $R/ai-cluster-gateway" >/dev/null

STATE=
for _ in $(seq 1 90); do
  STATE=$(CURL_MAX=5 net_curl -H "Authorization: Bearer $KEY" http://aic-gateway-r:8080/v1/models | grep -o '"state":"[a-z]*"')
  [ "$STATE" = '"state":"ready"' ] && break
  sleep 3
done
echo "estado: $STATE"
[ "$STATE" = '"state":"ready"' ] || fail "o modelo deveria ficar pronto"
echo "OK: modelo pronto"

echo "=== B) chat sem stream e com stream ==="
BODY='{"model":"smollm2-q4","messages":[{"role":"user","content":"The capital of France is"}],"max_tokens":8,"temperature":0}'
OUT=$(net_curl -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" -d "$BODY" http://aic-gateway-r:8080/v1/chat/completions)
echo "$OUT" | cut -c1-300
echo "$OUT" | grep -q '"content"' || fail "o chat deveria responder"
echo "$OUT" | grep -q '"total_tokens"' || fail "a resposta deveria trazer usage"
STREAM=$(net_curl -N -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
  -d '{"model":"smollm2-q4","messages":[{"role":"user","content":"Hello"}],"max_tokens":6,"stream":true}' \
  http://aic-gateway-r:8080/v1/chat/completions)
echo "$STREAM" | grep -q "^data: \[DONE\]" || fail "o stream deveria terminar com [DONE]"
echo "$STREAM" | grep -q '"total_tokens"' || fail "o stream deveria trazer o usage final"
echo "OK: chat e stream"

echo "=== C) autenticação e modelo desconhecido ==="
CODE=$(net_curl -o /dev/null -w "%{http_code}" -H "Authorization: Bearer errada" -H "Content-Type: application/json" -d "$BODY" http://aic-gateway-r:8080/v1/chat/completions)
[ "$CODE" = 401 ] || fail "chave errada deveria dar 401 (deu $CODE)"
CODE=$(net_curl -o /dev/null -w "%{http_code}" -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" -d '{"model":"nao-existe","messages":[{"role":"user","content":"oi"}]}' http://aic-gateway-r:8080/v1/chat/completions)
[ "$CODE" = 404 ] || fail "modelo desconhecido deveria dar 404 (deu $CODE)"
echo "OK: 401 e 404"

echo "=== D) o worker usou o rpc-server e a porta local não é alcançável ==="
docker logs aic-worker-r 2>&1 | grep -q "Accepted client connection" || fail "o rpc-server do worker deveria ter aceitado o gateway"
PORT=$(CURL_MAX=4 net_curl -o /dev/null -w "%{http_code}" http://aic-worker-r:50062/ ; true)
[ "$PORT" = 000 ] || fail "a porta 50062 (rpc-server) não deveria ser alcançável de fora (deu $PORT)"
echo "OK: rpc-server usado e escondido"

echo "=== E) texto puro e cliente sem certificado não chegam ao rpc-server ==="
BEFORE=$(docker logs aic-worker-r 2>&1 | grep -c "Accepted client connection")
CURL_MAX=4 net_curl -o /dev/null http://aic-worker-r:50061/ ; true
CURL_MAX=4 net_curl -k -o /dev/null https://aic-worker-r:50061/ ; true
sleep 1
AFTER=$(docker logs aic-worker-r 2>&1 | grep -c "Accepted client connection")
[ "$BEFORE" = "$AFTER" ] || fail "texto puro ou TLS sem certificado de cliente chegou ao rpc-server"
echo "OK: o túnel exige mTLS"

teardown
rm -rf "$PKI"
echo "=== llama.cpp por RPC sobre mTLS OK ==="
