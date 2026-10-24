#!/usr/bin/env bash
# Fatia da Fase J (J3): /v1/chat/completions sobre um LLM local, dentro do gateway.
# Uso: phase-j-chat.sh [gpt2|smollm2]   (um modelo por vez, para limitar a memória)
#
# Critério verificado: com os pesos montados, o gateway lista o modelo `gpt2` e devolve uma
# continuação determinística no formato de chat; sem chave responde 401; erros de entrada têm
# códigos estáveis. Os pesos não ficam no repositório: rode a partir de code/ com
# code/models/gpt2 já baixado (veja implementation/inference-engine.md).
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
JOIN=c-token
OPERATOR=c-operator
KEY=c-api-key
CONTROLLER=http://aic-controller:50051
GATEWAY=http://aic-gateway:8080

case "${1:-gpt2}" in
  gpt2) ID=gpt2; ENVVAR=AI_CLUSTER_GPT2_DIR; DIR=models/gpt2 ;;
  smollm2) ID=smollm2-135m; ENVVAR=AI_CLUSTER_SMOLLM2_DIR; DIR=models/smollm2 ;;
  *) echo "uso: $0 [gpt2|smollm2]"; exit 1 ;;
esac
[ -f "$DIR/model.safetensors" ] || { echo "FALHA: faltam os pesos em code/$DIR"; exit 1; }

teardown() {
  docker rm -f aic-controller aic-gateway >/dev/null 2>&1
}

fail() {
  echo "FALHA: $1"
  docker logs aic-gateway 2>&1 | tail -5
  teardown
  exit 1
}

common=(--network "$NET" -v aic-target:/t -e AI_CLUSTER_INSECURE=1
  -e AI_CLUSTER_JOIN_TOKEN=$JOIN -e AI_CLUSTER_OPERATOR_TOKEN=$OPERATOR)

http() {
  MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" "$IMG" curl -s "$@"
}

chat() { # chat <corpo json> -> resposta
  http -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" -d "$1" \
    --max-time 60 $GATEWAY/v1/chat/completions
}

status() { # status <corpo json> -> código HTTP
  http -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $KEY" \
    -H "Content-Type: application/json" -d "$1" --max-time 60 $GATEWAY/v1/chat/completions
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

MSYS_NO_PATHCONV=1 docker run -d --name aic-controller "${common[@]}" -e AI_CLUSTER_AUTO_APPROVE=1 \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
sleep 2
MSYS_NO_PATHCONV=1 docker run -d --name aic-gateway "${common[@]}" -e AI_CLUSTER_API_KEY=$KEY \
  -v "$PWD/$DIR:/models:ro" -e $ENVVAR=/models \
  -e AI_CLUSTER_GATEWAY_ADDR=0.0.0.0:8080 -e AI_CLUSTER_CONTROLLER=$CONTROLLER \
  "$IMG" sh -c "exec $R/ai-cluster-gateway" >/dev/null

# O gateway só abre a porta depois de carregar o modelo.
for _ in $(seq 1 60); do
  http $GATEWAY/health | grep -q green && break
  sleep 2
done
http $GATEWAY/health | grep -q green || fail "o gateway não subiu"

echo "=== A) o modelo aparece em /v1/models ==="
MODELS=$(http -H "Authorization: Bearer $KEY" $GATEWAY/v1/models)
echo "$MODELS" | grep -q "\"id\":\"$ID\"" || fail "$ID deveria estar listado: $MODELS"
echo "OK: $ID listado"

echo "=== B) uma continuação em formato de chat ==="
BODY="{\"model\":\"$ID\",\"max_tokens\":5,\"messages\":[{\"role\":\"user\",\"content\":\"The capital of France is\"}]}"
ONE=$(chat "$BODY")
echo "$ONE"
echo "$ONE" | grep -q '"object":"chat.completion"' || fail "faltou o objeto chat.completion"
echo "$ONE" | grep -q '"finish_reason":"length"' || fail "com 5 tokens a parada deveria ser por tamanho"
echo "$ONE" | grep -q '"completion_tokens":5' || fail "deveria ter gerado 5 tokens"
TWO=$(chat "$BODY")
# O id e o horário mudam a cada pedido; o resto da resposta (o texto) não.
strip() { echo "$1" | sed -E 's/"id":"chatcmpl-[0-9]+"//; s/"created":[0-9]+//'; }
[ "$(strip "$ONE")" = "$(strip "$TWO")" ] || fail "a mesma entrada deveria dar a mesma continuação"
echo "OK: formato de chat, 5 tokens, determinístico"

echo "=== C) autenticação e erros ==="
CODE=$(http -o /dev/null -w '%{http_code}' -H "Content-Type: application/json" -d "$BODY" $GATEWAY/v1/chat/completions)
[ "$CODE" = 401 ] || fail "sem chave deveria dar 401, deu: $CODE"
[ "$(status '{"model":"outro","messages":[{"role":"user","content":"oi"}]}')" = 404 ] || fail "modelo desconhecido deveria dar 404"
[ "$(status "{\"model\":\"$ID\",\"messages\":[]}")" = 400 ] || fail "sem mensagens deveria dar 400"
[ "$(status "{\"model\":\"$ID\",\"messages\":[{\"role\":\"robot\",\"content\":\"oi\"}]}")" = 400 ] || fail "role inválido deveria dar 400"
[ "$(status "{\"model\":\"$ID\",\"stream\":true,\"messages\":[{\"role\":\"user\",\"content\":\"oi\"}]}")" = 400 ] || fail "stream deveria dar 400"
[ "$(status 'não é json')" = 400 ] || fail "corpo inválido deveria dar 400"
echo "OK: 401, 404 e 400"

teardown
echo "=== J3 chat completions OK ==="
