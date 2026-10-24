#!/usr/bin/env bash
# Fatia da Fase J (J1): Inference API sobre um modelo sintético.
#
# Critério verificado: o gateway HTTP autentica por chave de API, lista o modelo, e devolve por
# POST /v1/embeddings um vetor que saiu de um grafo de multiplicação de matrizes executado nos
# workers — o mesmo texto dá o mesmo vetor, textos diferentes dão vetores diferentes — com erros
# estáveis para chave ruim, modelo desconhecido, corpo grande demais e entrada inválida.
#
# Sequência:
#   A) o gateway não sobe sem chave, nem com a chave igual ao token de join.
#   B) sem chave: 401; com chave: /v1/models lista o modelo marcado como sintético.
#   C) embeddings: 16 números, mesmo texto => mesmo vetor, outro texto => outro vetor.
#   D) modelo desconhecido: 404; corpo grande: 413; entrada vazia: 400.
#   E) sem worker o pedido estoura o prazo e responde 504 (não pendura).
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
JOIN=j-token
OPERATOR=j-operator
KEY=j-api-key
CONTROLLER=http://aic-controller:50051
GATEWAY=http://aic-gateway:8080

teardown() {
  docker rm -f aic-controller aic-worker-1 aic-worker-2 aic-gateway >/dev/null 2>&1
}

fail() {
  echo "FALHA: $1"
  teardown
  exit 1
}

common=(--network "$NET" -v aic-target:/t -e AI_CLUSTER_INSECURE=1
  -e AI_CLUSTER_JOIN_TOKEN=$JOIN -e AI_CLUSTER_OPERATOR_TOKEN=$OPERATOR)

# http <args do curl>: roda curl numa rede do cluster.
http() {
  MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" "$IMG" curl -s "$@"
}

embed() { # embed <texto> -> JSON
  http -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
    -d "{\"model\":\"matmul-diamond-v0\",\"input\":\"$1\"}" $GATEWAY/v1/embeddings
}

vector() { # vector <json> -> os números do embedding, um por linha
  echo "$1" | sed -E 's/.*"embedding":\[([^]]*)\].*/\1/' | tr ',' '\n'
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

echo "=== A) o gateway exige uma chave própria ==="
OUT=$(MSYS_NO_PATHCONV=1 docker run --rm "${common[@]}" "$IMG" sh -c "$R/ai-cluster-gateway" 2>&1)
echo "$OUT" | grep -q "AI_CLUSTER_API_KEY" || fail "sem chave o gateway deveria recusar subir, saiu: $OUT"
OUT=$(MSYS_NO_PATHCONV=1 docker run --rm "${common[@]}" -e AI_CLUSTER_API_KEY=$JOIN "$IMG" \
  sh -c "$R/ai-cluster-gateway" 2>&1)
echo "$OUT" | grep -q "diferente" || fail "a chave igual ao token de join deveria ser recusada, saiu: $OUT"
echo "OK: o gateway recusou subir sem chave e com a chave igual ao token de join"

MSYS_NO_PATHCONV=1 docker run -d --name aic-controller "${common[@]}" -e AI_CLUSTER_AUTO_APPROVE=1 \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
sleep 2
MSYS_NO_PATHCONV=1 docker run -d --name aic-gateway "${common[@]}" -e AI_CLUSTER_API_KEY=$KEY \
  -e AI_CLUSTER_GATEWAY_ADDR=0.0.0.0:8080 -e AI_CLUSTER_CONTROLLER=$CONTROLLER \
  "$IMG" sh -c "exec $R/ai-cluster-gateway" >/dev/null
sleep 2

echo "=== E) sem worker o pedido estoura o prazo ==="
# Roda primeiro, com o cluster sem workers. O prazo do gateway é de 30 s.
CODE=$(http -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" -d '{"model":"matmul-diamond-v0","input":"sem worker"}' \
  --max-time 45 $GATEWAY/v1/embeddings)
[ "$CODE" = 504 ] || fail "sem worker o gateway deveria responder 504, respondeu: $CODE"
echo "OK: sem worker a resposta foi 504"

for i in 1 2; do
  MSYS_NO_PATHCONV=1 docker run -d --name "aic-worker-$i" "${common[@]}" \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID="worker-$i" \
    "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
done
sleep 8

echo "=== B) autenticação e /v1/models ==="
CODE=$(http -o /dev/null -w '%{http_code}' $GATEWAY/v1/models)
[ "$CODE" = 401 ] || fail "/v1/models sem chave deveria responder 401, respondeu: $CODE"
CODE=$(http -o /dev/null -w '%{http_code}' -H "Authorization: Bearer errada" $GATEWAY/v1/models)
[ "$CODE" = 401 ] || fail "/v1/models com chave errada deveria responder 401, respondeu: $CODE"
MODELS=$(http -H "Authorization: Bearer $KEY" $GATEWAY/v1/models)
echo "$MODELS"
echo "$MODELS" | grep -q '"synthetic-matmul"' || fail "o modelo deveria vir marcado como sintético"
http $GATEWAY/health | grep -q green || fail "/health deveria responder sem chave"
echo "OK: 401 sem chave, modelo sintético listado, /health aberto"

echo "=== C) embeddings ==="
ONE=$(embed "primeiro texto")
echo "$ONE" | cut -c1-200
[ "$(vector "$ONE" | wc -l)" = 16 ] || fail "o vetor deveria ter 16 números"
echo "$ONE" | grep -q '"synthetic":true' || fail "a resposta deveria declarar synthetic"
TWO=$(embed "primeiro texto")
OTHER=$(embed "outro texto")
[ "$(vector "$ONE")" = "$(vector "$TWO")" ] || fail "o mesmo texto deveria dar o mesmo vetor"
[ "$(vector "$ONE")" != "$(vector "$OTHER")" ] || fail "textos diferentes deveriam dar vetores diferentes"
HEADERS=$(http -D - -o /dev/null -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
  -d '{"model":"matmul-diamond-v0","input":"cabeçalhos"}' $GATEWAY/v1/embeddings)
echo "$HEADERS" | grep -qi '^x-request-id: req-' || fail "faltou o cabeçalho x-request-id"
echo "$HEADERS" | grep -qi '^x-model-id: matmul-diamond-v0' || fail "faltou o cabeçalho x-model-id"
echo "OK: 16 números, determinístico, diferente para outro texto, cabeçalhos de correlação"

echo "=== D) erros estáveis ==="
status() { # status <corpo> -> código HTTP
  http -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
    -d "$1" $GATEWAY/v1/embeddings
}
[ "$(status '{"model":"gpt-x","input":"a"}')" = 404 ] || fail "modelo desconhecido deveria dar 404"
[ "$(status '{"model":"matmul-diamond-v0","input":""}')" = 400 ] || fail "entrada vazia deveria dar 400"
[ "$(status 'isto não é json')" = 400 ] || fail "corpo que não é JSON deveria dar 400"
# O corpo de 70 KB é montado dentro do contêiner: na linha de comando estouraria o limite de
# argumentos do Docker no Windows.
BIGCMD='{ printf "{\"model\":\"matmul-diamond-v0\",\"input\":\""; head -c 70000 /dev/zero | tr "\0" a; printf "\"}"; } | curl -s -o /dev/null -w "%{http_code}" -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" --data-binary @- $GATEWAY/v1/embeddings'
CODE=$(MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" -e KEY=$KEY -e GATEWAY=$GATEWAY "$IMG" sh -c "$BIGCMD")
[ "$CODE" = 413 ] || fail "corpo grande deveria dar 413, deu: $CODE"
echo "OK: 404, 400, 400 e 413"

teardown
echo "=== J1 Inference API OK ==="
