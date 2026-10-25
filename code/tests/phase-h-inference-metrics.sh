#!/usr/bin/env bash
# Fatia da Fase H (F05): métricas de geração no gateway.
#
# Critério verificado: depois de três gerações (duas inteiras e uma em streaming), o
# `GET /metrics` do gateway mostra requisições, tokens, TTFT, ITL, tokens por segundo e o
# tamanho do KV cache; sem a chave responde 401. Exige code/models/gpt2.
set -uo pipefail

NET=aic-net-h5
IMG=aic-rust:1
R=/t/release
JOIN=h-token
OPERATOR=h-operator
KEY=h-api-key
CONTROLLER=http://aic-controller-h5:50051
GATEWAY=http://aic-gateway-h5:8080
DIR=models/gpt2
[ -f "$DIR/model.safetensors" ] || { echo "FALHA: faltam os pesos em code/$DIR"; exit 1; }

teardown() { docker rm -f aic-controller-h5 aic-gateway-h5 >/dev/null 2>&1; }
fail() { echo "FALHA: $1"; docker logs aic-gateway-h5 2>&1 | tail -5; teardown; exit 1; }

common=(--network "$NET" -v aic-target:/t -e AI_CLUSTER_INSECURE=1
  -e AI_CLUSTER_JOIN_TOKEN=$JOIN -e AI_CLUSTER_OPERATOR_TOKEN=$OPERATOR)

http() { MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" "$IMG" curl -s "$@"; }
auth=(-H "Authorization: Bearer $KEY")

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null
MSYS_NO_PATHCONV=1 docker run -d --name aic-controller-h5 "${common[@]}" -e AI_CLUSTER_AUTO_APPROVE=1 \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
sleep 2
MSYS_NO_PATHCONV=1 docker run -d --name aic-gateway-h5 "${common[@]}" -e AI_CLUSTER_API_KEY=$KEY \
  -v "$PWD/$DIR:/models:ro" -e AI_CLUSTER_GPT2_DIR=/models -e AI_CLUSTER_CACHE_ENTRIES=0 \
  -e AI_CLUSTER_GATEWAY_ADDR=0.0.0.0:8080 -e AI_CLUSTER_CONTROLLER=$CONTROLLER \
  "$IMG" sh -c "exec $R/ai-cluster-gateway" >/dev/null
for _ in $(seq 1 60); do http $GATEWAY/health | grep -q green && break; sleep 2; done
http $GATEWAY/health | grep -q green || fail "o gateway não subiu"

BODY='{"model":"gpt2","max_tokens":5,"messages":[{"role":"user","content":"The capital of France is"}]}'
echo "=== A) três gerações: duas inteiras e uma em streaming ==="
for _ in 1 2; do
  http "${auth[@]}" -H "Content-Type: application/json" -d "$BODY" --max-time 60 $GATEWAY/v1/chat/completions \
    | grep -q '"completion_tokens":5' || fail "a geração deveria dar 5 tokens"
done
http -N "${auth[@]}" -H "Content-Type: application/json" --max-time 60 \
  -d "${BODY%\}*},\"stream\":true}" $GATEWAY/v1/chat/completions | grep -q '^data: \[DONE\]' || fail "faltou [DONE]"
sleep 1
echo "OK: três gerações"

echo "=== B) /metrics mostra as séries de geração ==="
[ "$(http -o /dev/null -w '%{http_code}' $GATEWAY/metrics)" = 401 ] || fail "sem chave deveria dar 401"
METRICS=$(http "${auth[@]}" $GATEWAY/metrics)
echo "$METRICS" | grep -v '^#'
value() { echo "$METRICS" | awk -v n="$1" '$1 == n { print $2 }'; }
positive() { awk -v v="$(value "$1")" 'BEGIN { exit !(v > 0) }'; }
[ "$(value ai_cluster_inference_requests_total)" = 3 ] || fail "esperado 3 requisições"
[ "$(value ai_cluster_inference_generated_tokens_total)" = 15 ] || fail "esperado 15 tokens gerados"
for series in ai_cluster_inference_ttft_seconds_p50 ai_cluster_inference_ttft_seconds_p95 \
  ai_cluster_inference_itl_seconds_p50 ai_cluster_inference_tokens_per_second \
  ai_cluster_inference_kv_cache_bytes_last ai_cluster_inference_kv_cache_bytes_max; do
  positive "$series" || fail "$series deveria ser positivo"
done
# gpt2: 12 camadas x 2 x 768 x 8 bytes por posição, e a geração termina com prompt + 5 posições.
[ "$(value ai_cluster_inference_kv_cache_bytes_last)" -gt 147456 ] || fail "o KV cache deveria passar de uma posição"
echo "OK: requisições, tokens, TTFT, ITL, tokens/s e KV cache"

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== fim ==="
