#!/usr/bin/env bash
# Fatia da Fase N (F12): manifest de modelo com versão e checksum dos pesos.
# Exige code/models/gpt2. Grava code/models/gpt2/manifest.json (ignorado pelo git) e cria e
# remove code/models/gpt2-bad (só links para os mesmos pesos e um manifest adulterado).
#
# Sequência:
#   A) `aic cluster model manifest` grava o manifest e `model verify` confere;
#   B) o gateway sobe com o manifest, lista a versão em /v1/models e a devolve no cabeçalho
#      `x-model-version` do chat;
#   C) um manifest que não confere com os pesos impede o gateway de subir, e nomeia o arquivo;
#   D) com AI_CLUSTER_REQUIRE_MANIFEST=1 um modelo sem manifest é recusado.
set -uo pipefail

NET=aic-net-n
IMG=aic-rust:1
R=/t/release
KEY=n-api-key
JOIN=n-join
OPERATOR=n-operator
DIR=models/gpt2
[ -f "$DIR/model.safetensors" ] || { echo "FALHA: faltam os pesos em code/$DIR"; exit 1; }

teardown() {
  docker rm -f aic-controller-n aic-gateway-n >/dev/null 2>&1
  rm -rf models/gpt2-bad
}
fail() { echo "FALHA: $1"; teardown; exit 1; }

common=(--network "$NET" -v aic-target:/t -e AI_CLUSTER_INSECURE=1
  -e AI_CLUSTER_JOIN_TOKEN=$JOIN -e AI_CLUSTER_OPERATOR_TOKEN=$OPERATOR)
http() { MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" "$IMG" curl -s "$@"; }
cli() { MSYS_NO_PATHCONV=1 docker run --rm -v "$PWD/models:/models" -v aic-target:/t "$IMG" sh -c "$R/aic $*" 2>&1; }

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

echo "=== A) gerar e conferir o manifest ==="
cli cluster model manifest /models/gpt2 --name gpt2 --version 124m-test-1 | tail -2
cli cluster model verify /models/gpt2 | grep -q "ok: gpt2 124m-test-1" || fail "model verify deveria conferir"
echo "OK"

start_gateway() { # start_gateway <diretório de pesos dentro do contêiner> [variáveis extras...]
  local dir=$1; shift
  docker rm -f aic-gateway-n >/dev/null 2>&1
  MSYS_NO_PATHCONV=1 docker run -d --name aic-gateway-n "${common[@]}" -e AI_CLUSTER_API_KEY=$KEY \
    -v "$PWD/models:/models:ro" -e AI_CLUSTER_GPT2_DIR="$dir" "$@" \
    -e AI_CLUSTER_GATEWAY_ADDR=0.0.0.0:8080 -e AI_CLUSTER_CONTROLLER=http://aic-controller-n:50051 \
    "$IMG" sh -c "exec $R/ai-cluster-gateway" >/dev/null
}
MSYS_NO_PATHCONV=1 docker run -d --name aic-controller-n "${common[@]}" -e AI_CLUSTER_AUTO_APPROVE=1 \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
sleep 2

echo "=== B) a versão aparece na API ==="
start_gateway /models/gpt2
for _ in $(seq 1 60); do http http://aic-gateway-n:8080/health | grep -q green && break; sleep 2; done
http http://aic-gateway-n:8080/health | grep -q green || fail "o gateway não subiu"
http -H "Authorization: Bearer $KEY" http://aic-gateway-n:8080/v1/models | grep -q '"version":"124m-test-1"' \
  || fail "/v1/models deveria trazer a versão"
HEADERS=$(http -D - -o /dev/null -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" --max-time 60 \
  -d '{"model":"gpt2","max_tokens":2,"messages":[{"role":"user","content":"hi"}]}' http://aic-gateway-n:8080/v1/chat/completions)
echo "$HEADERS" | grep -qi '^x-model-version: 124m-test-1' || fail "o chat deveria devolver x-model-version: $HEADERS"
echo "OK: versão 124m-test-1 em /v1/models e em x-model-version"

echo "=== C) manifest que não confere com os pesos ==="
mkdir -p models/gpt2-bad
for f in config.json merges.txt vocab.json model.safetensors; do ln -s "../gpt2/$f" "models/gpt2-bad/$f"; done
# Um hash adulterado: o dono do manifest diz que o arquivo é outro.
sed 's/"sha256": "[0-9a-f]\{4\}/"sha256": "0000/' models/gpt2/manifest.json > models/gpt2-bad/manifest.json
start_gateway /models/gpt2-bad
sleep 8
LOG=$(docker logs aic-gateway-n 2>&1)
echo "$LOG" | tail -3
echo "$LOG" | grep -q "não confere com o manifest" || fail "o gateway deveria recusar subir com o manifest errado"
docker ps -q -f name=aic-gateway-n | grep -q . && fail "o gateway não deveria estar rodando"
echo "OK: o gateway recusou os pesos que não conferem"

echo "=== D) manifest exigido ==="
rm -f models/gpt2-bad/manifest.json
start_gateway /models/gpt2-bad -e AI_CLUSTER_REQUIRE_MANIFEST=1
sleep 6
docker logs aic-gateway-n 2>&1 | grep -q "sem manifest.json (exigido)" || fail "deveria recusar modelo sem manifest quando exigido"
echo "OK: recusado sem manifest"

teardown
docker network rm "$NET" >/dev/null 2>&1
rm -f models/gpt2/manifest.json
echo "=== fim ==="
