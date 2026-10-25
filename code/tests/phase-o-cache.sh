#!/usr/bin/env bash
# Fatia da Fase O (F14, M1): cache exato de respostas no gateway. Exige code/models/gpt2.
#
# Critério verificado: o mesmo pedido repetido é respondido da memória (mais rápido e com o
# mesmo texto), em resposta inteira e em streaming; outro max_tokens não acerta o cache; o
# /metrics mostra acertos, erros e tokens poupados.
set -uo pipefail

NET=aic-net-o
IMG=aic-rust:1
R=/t/release
KEY=o-api-key
JOIN=o-join
OPERATOR=o-operator
DIR=models/gpt2
[ -f "$DIR/model.safetensors" ] || { echo "FALHA: faltam os pesos em code/$DIR"; exit 1; }

teardown() { docker rm -f aic-controller-o aic-gateway-o >/dev/null 2>&1; }
fail() { echo "FALHA: $1"; docker logs aic-gateway-o 2>&1 | tail -5; teardown; exit 1; }

common=(--network "$NET" -v aic-target:/t -e AI_CLUSTER_INSECURE=1
  -e AI_CLUSTER_JOIN_TOKEN=$JOIN -e AI_CLUSTER_OPERATOR_TOKEN=$OPERATOR)
http() { MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" "$IMG" curl -s "$@"; }
G=http://aic-gateway-o:8080

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null
MSYS_NO_PATHCONV=1 docker run -d --name aic-controller-o "${common[@]}" -e AI_CLUSTER_AUTO_APPROVE=1 \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
sleep 2
MSYS_NO_PATHCONV=1 docker run -d --name aic-gateway-o "${common[@]}" -e AI_CLUSTER_API_KEY=$KEY \
  -v "$PWD/$DIR:/models:ro" -e AI_CLUSTER_GPT2_DIR=/models \
  -e AI_CLUSTER_GATEWAY_ADDR=0.0.0.0:8080 -e AI_CLUSTER_CONTROLLER=http://aic-controller-o:50051 \
  "$IMG" sh -c "exec $R/ai-cluster-gateway" >/dev/null
for _ in $(seq 1 60); do http $G/health | grep -q green && break; sleep 2; done
http $G/health | grep -q green || fail "o gateway não subiu"

body() { echo "{\"model\":\"gpt2\",\"max_tokens\":$1,\"messages\":[{\"role\":\"user\",\"content\":\"The capital of France is\"}]}"; }
ask() { # ask <max_tokens> -> "tempo_s|corpo"
  http -w '|%{time_total}' -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" --max-time 60 -d "$(body "$1")" $G/v1/chat/completions
}
content_of() { echo "$1" | grep -o '"content":"[^"]*"' | head -1; }

echo "=== A) o segundo pedido igual vem da memória ==="
MISS=$(ask 20); MISS_T=${MISS##*|}
HIT=$(ask 20); HIT_T=${HIT##*|}
echo "sem cache: ${MISS_T}s   com cache: ${HIT_T}s"
[ "$(content_of "$MISS")" = "$(content_of "$HIT")" ] || fail "o texto do acerto deveria ser igual ao da geração"
awk -v miss="$MISS_T" -v hit="$HIT_T" 'BEGIN { exit !(hit * 3 < miss) }' || fail "o acerto deveria ser pelo menos 3x mais rápido"
echo "OK: mesmo texto, ${HIT_T}s contra ${MISS_T}s"

echo "=== B) streaming também acerta e entrega o mesmo texto ==="
SSE=$(http -N -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" --max-time 60 \
  -d "$(body 20 | sed 's/}$/,"stream":true}/')" $G/v1/chat/completions)
echo "$SSE" | grep -q '^data: \[DONE\]' || fail "faltou [DONE] no streaming"
JOINED=$(echo "$SSE" | grep -o '"content":"[^"]*"' | sed 's/"content":"//; s/"$//' | tr -d '\n')
WHOLE=$(content_of "$MISS" | sed 's/"content":"//; s/"$//')
[ "$JOINED" = "$WHOLE" ] || fail "o texto do streaming ($JOINED) deveria ser igual ao inteiro ($WHOLE)"
echo "OK: streaming idêntico ao texto inteiro"

echo "=== C) outro max_tokens é outro pedido ==="
OTHER=$(ask 7); OTHER_T=${OTHER##*|}
awk -v t="$OTHER_T" 'BEGIN { exit !(t > 0.15) }' || fail "outro max_tokens deveria gerar de novo (levou ${OTHER_T}s)"
echo "OK: ${OTHER_T}s (gerou de novo)"

echo "=== D) /metrics ==="
METRICS=$(http -H "Authorization: Bearer $KEY" $G/metrics)
echo "$METRICS" | grep -E '^ai_cluster_cache_'
value() { echo "$METRICS" | awk -v n="$1" '$1 == n { print $2 }'; }
[ "$(value ai_cluster_cache_hits_total)" = 2 ] || fail "esperado 2 acertos (inteiro e streaming)"
[ "$(value ai_cluster_cache_misses_total)" = 2 ] || fail "esperado 2 erros (20 e 7 tokens)"
[ "$(value ai_cluster_cache_tokens_avoided_total)" = 40 ] || fail "esperado 40 tokens poupados"
[ "$(value ai_cluster_inference_requests_total)" = 2 ] || fail "só as 2 gerações reais entram nas métricas de geração"
echo "OK: 2 acertos, 2 erros, 40 tokens poupados; só gerações reais nas métricas de TTFT"

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== fim ==="
