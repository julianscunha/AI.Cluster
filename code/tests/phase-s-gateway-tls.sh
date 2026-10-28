#!/usr/bin/env bash
# Perna HTTP do gateway com TLS: com certificado e chave o gateway serve HTTPS e recusa texto puro;
# sem eles, fora de loopback e sem AI_CLUSTER_INSECURE, ele nem sobe. Precisa de code/models/gpt2.
set -uo pipefail
NET=aic-net
IMG=aic-rust:1
R=/t/release
KEY=c-api-key
DIR=models/gpt2
[ -f "$DIR/model.safetensors" ] || { echo "FALHA: faltam os pesos em code/$DIR"; exit 1; }

teardown() { docker rm -f aic-controller aic-gateway >/dev/null 2>&1; docker volume rm aic-tls >/dev/null 2>&1; }
fail() { echo "FALHA: $1"; docker logs aic-gateway 2>&1 | tail -5; teardown; exit 1; }
run() { MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" -v aic-tls:/tls "$IMG" "$@"; }

teardown
docker network rm "$NET" >/dev/null 2>&1; docker network create "$NET" >/dev/null
docker volume create aic-tls >/dev/null

echo "=== A) certificado autoassinado para o nome aic-gateway ==="
run sh -c 'openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj /CN=aic-gateway -addext subjectAltName=DNS:aic-gateway -keyout /tls/key.pem -out /tls/cert.pem 2>/dev/null && chmod 644 /tls/*.pem' \
  || fail "não consegui gerar o certificado"

common=(--network "$NET" -v aic-target:/t -v aic-tls:/tls:ro -e AI_CLUSTER_INSECURE=1
  -e AI_CLUSTER_JOIN_TOKEN=c-token -e AI_CLUSTER_OPERATOR_TOKEN=c-operator)
MSYS_NO_PATHCONV=1 docker run -d --name aic-controller "${common[@]}" -e AI_CLUSTER_AUTO_APPROVE=1 \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
sleep 2

echo "=== B) só o certificado, sem a chave: não sobe ==="
OUT=$(MSYS_NO_PATHCONV=1 docker run --rm "${common[@]}" -e AI_CLUSTER_API_KEY=$KEY \
  -v "$PWD/$DIR:/models:ro" -e AI_CLUSTER_GPT2_DIR=/models -e AI_CLUSTER_GATEWAY_ADDR=0.0.0.0:8080 \
  -e AI_CLUSTER_CONTROLLER=http://aic-controller:50051 -e AI_CLUSTER_GATEWAY_TLS_CERT=/tls/cert.pem \
  "$IMG" sh -c "$R/ai-cluster-gateway" 2>&1)
echo "$OUT" | grep -q "vêm juntos" || fail "deveria recusar certificado sem chave: $OUT"
echo "OK: recusou"

echo "=== C) fora de loopback sem TLS e sem INSECURE: não sobe ==="
OUT=$(MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" -v aic-target:/t -e AI_CLUSTER_API_KEY=$KEY \
  -e AI_CLUSTER_JOIN_TOKEN=c-token -e AI_CLUSTER_OPERATOR_TOKEN=c-operator \
  -v "$PWD/$DIR:/models:ro" -e AI_CLUSTER_GPT2_DIR=/models -e AI_CLUSTER_GATEWAY_ADDR=0.0.0.0:8080 \
  -e AI_CLUSTER_CONTROLLER=http://aic-controller:50051 \
  "$IMG" sh -c "$R/ai-cluster-gateway" 2>&1)
echo "$OUT" | grep -q "não tem TLS" || fail "deveria recusar sem TLS: $OUT"
echo "OK: recusou"

echo "=== D) com certificado e chave: HTTPS responde, texto puro não ==="
MSYS_NO_PATHCONV=1 docker run -d --name aic-gateway "${common[@]}" -e AI_CLUSTER_API_KEY=$KEY \
  -v "$PWD/$DIR:/models:ro" -e AI_CLUSTER_GPT2_DIR=/models -e AI_CLUSTER_GATEWAY_ADDR=0.0.0.0:8080 \
  -e AI_CLUSTER_CONTROLLER=http://aic-controller:50051 \
  -e AI_CLUSTER_GATEWAY_TLS_CERT=/tls/cert.pem -e AI_CLUSTER_GATEWAY_TLS_KEY=/tls/key.pem \
  "$IMG" sh -c "exec $R/ai-cluster-gateway" >/dev/null
for _ in $(seq 1 60); do
  run curl -s --cacert /tls/cert.pem https://aic-gateway:8080/health | grep -q green && break
  sleep 2
done
run curl -s --cacert /tls/cert.pem https://aic-gateway:8080/health | grep -q green || fail "o HTTPS não respondeu"
CODE=$(run curl -s -o /dev/null -w '%{http_code}' --cacert /tls/cert.pem -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" -d '{"model":"gpt2","max_tokens":2,"messages":[{"role":"user","content":"Hello"}]}' \
  https://aic-gateway:8080/v1/chat/completions)
[ "$CODE" = 200 ] || fail "chat por HTTPS deveria dar 200, deu: $CODE"
PLAIN=$(run curl -s -o /dev/null -w '%{http_code}' --max-time 10 http://aic-gateway:8080/health)
[ "$PLAIN" != 200 ] || fail "texto puro não deveria ser atendido"
UNTRUSTED=$(run curl -s -o /dev/null -w '%{http_code}' --max-time 10 https://aic-gateway:8080/health)
[ "$UNTRUSTED" != 200 ] || fail "sem a CA o cliente deveria recusar o certificado"
echo "OK: HTTPS 200, texto puro recusado, certificado não confiável recusado"

teardown
echo "=== TLS do gateway OK ==="
