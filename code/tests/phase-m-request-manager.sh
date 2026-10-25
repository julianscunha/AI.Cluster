#!/usr/bin/env bash
# Fatia da Fase M (F11): Request Manager no gateway — tenants, ACL de modelo, cota de tokens,
# fila por classe e cancelamento do grafo que estourou o prazo. Exige code/models/gpt2.
#
# Sequência:
#   A) chaves e ACL: cada tenant vê e chama só os modelos que tem; chave inventada dá 401;
#   B) cota: depois de gastar os tokens da janela o tenant recebe 429 QUOTA_EXCEEDED com Retry-After;
#   C) fila por classe: com a geração ocupada, o pedido interactive que chegou depois termina
#      antes do batch que chegou primeiro;
#   D) grafo abandonado: embeddings sem worker estouram o prazo (504) e o controller cancela o
#      grafo (evento request_cancelled).
set -uo pipefail

NET=aic-net-m
IMG=aic-rust:1
R=/t/release
JOIN=m-join
OPERATOR=m-operator
CONTROLLER=http://aic-controller-m:50051
GATEWAY=http://aic-gateway-m:8080
DIR=models/gpt2
[ -f "$DIR/model.safetensors" ] || { echo "FALHA: faltam os pesos em code/$DIR"; exit 1; }

teardown() { docker rm -f aic-controller-m aic-gateway-m aic-curl-m1 aic-curl-m2 aic-curl-m3 >/dev/null 2>&1; docker volume rm aic-order-m >/dev/null 2>&1; }
fail() { echo "FALHA: $1"; docker logs aic-gateway-m 2>&1 | tail -5; teardown; exit 1; }

common=(--network "$NET" -v aic-target:/t -e AI_CLUSTER_INSECURE=1
  -e AI_CLUSTER_JOIN_TOKEN=$JOIN -e AI_CLUSTER_OPERATOR_TOKEN=$OPERATOR)

http() { MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" "$IMG" curl -s "$@"; }
chat() { # chat <chave> <max_tokens> [cabeçalhos extras...] -> corpo; o código HTTP vai na última linha
  local key=$1 max=$2; shift 2
  http -w '\n%{http_code}' -H "Authorization: Bearer $key" -H "Content-Type: application/json" "$@" --max-time 90 \
    -d "{\"model\":\"gpt2\",\"max_tokens\":$max,\"messages\":[{\"role\":\"user\",\"content\":\"The capital of France is\"}]}" \
    $GATEWAY/v1/chat/completions
}
code_of() { echo "$1" | tail -1; }

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null
docker volume create aic-order-m >/dev/null
MSYS_NO_PATHCONV=1 docker run -d --name aic-controller-m "${common[@]}" -e AI_CLUSTER_AUTO_APPROVE=1 \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
sleep 2
TENANTS='acme=ka,models=gpt2,tokens=30,class=interactive;beta=kb,models=matmul-diamond-v0;hi=kh,models=gpt2,class=interactive;lo=kl,models=gpt2,class=batch;big=kg,models=gpt2'
MSYS_NO_PATHCONV=1 docker run -d --name aic-gateway-m "${common[@]}" -e AI_CLUSTER_TENANTS="$TENANTS" \
  -v "$PWD/$DIR:/models:ro" -e AI_CLUSTER_GPT2_DIR=/models \
  -e AI_CLUSTER_GATEWAY_ADDR=0.0.0.0:8080 -e AI_CLUSTER_CONTROLLER=$CONTROLLER \
  "$IMG" sh -c "exec $R/ai-cluster-gateway" >/dev/null
for _ in $(seq 1 60); do http $GATEWAY/health | grep -q green && break; sleep 2; done
http $GATEWAY/health | grep -q green || fail "o gateway não subiu"

echo "=== A) chaves e ACL de modelo ==="
ACME=$(http -H "Authorization: Bearer ka" $GATEWAY/v1/models)
BETA=$(http -H "Authorization: Bearer kb" $GATEWAY/v1/models)
echo "$ACME" | grep -q '"id":"gpt2"' && ! echo "$ACME" | grep -q 'matmul-diamond' || fail "acme deveria ver só gpt2: $ACME"
echo "$BETA" | grep -q 'matmul-diamond' && ! echo "$BETA" | grep -q '"id":"gpt2"' || fail "beta deveria ver só matmul-diamond: $BETA"
[ "$(code_of "$(chat kb 3)")" = 403 ] || fail "beta chamando gpt2 deveria dar 403"
EMB=$(http -o /dev/null -w '%{http_code}' -H "Authorization: Bearer ka" -H "Content-Type: application/json" \
  -d '{"model":"matmul-diamond-v0","input":"oi"}' $GATEWAY/v1/embeddings)
[ "$EMB" = 403 ] || fail "acme chamando embeddings deveria dar 403, deu $EMB"
[ "$(code_of "$(chat inventada 3)")" = 401 ] || fail "chave inventada deveria dar 401"
echo "OK: ACL por tenant, 403 fora dela, 401 sem chave"

echo "=== B) cota de tokens ==="
# acme tem 30 tokens por janela; cada pedido (prompt de 5 + 5 gerados) gasta ~10.
for i in 1 2 3; do
  [ "$(code_of "$(chat ka 5)")" = 200 ] || fail "o pedido $i de acme deveria passar"
done
OUT=$(chat ka 5 -D /dev/stderr 2>&1)
[ "$(code_of "$OUT")" = 429 ] || fail "o quarto pedido de acme deveria dar 429: $OUT"
echo "$OUT" | grep -q QUOTA_EXCEEDED || fail "o 429 deveria ser QUOTA_EXCEEDED: $OUT"
echo "$OUT" | grep -qi '^retry-after:' || fail "o 429 deveria trazer Retry-After"
# Outro tenant não é afetado.
[ "$(code_of "$(chat kh 3)")" = 200 ] || fail "hi deveria seguir atendido"
METRICS=$(http -H "Authorization: Bearer ka" $GATEWAY/metrics)
echo "$METRICS" | grep 'ai_cluster_tenant_tokens_total{tenant="acme"}'
echo "$METRICS" | grep 'ai_cluster_tenant_rejected_total{tenant="acme"}' | grep -qv ' 0$' || fail "acme deveria ter rejeições contadas"
echo "OK: 429 QUOTA_EXCEEDED com Retry-After; outro tenant segue; contadores no /metrics"

echo "=== C) fila por classe ==="
# Subir um contêiner leva alguns segundos e varia; por isso cada pedido espera um instante
# combinado, contado de um mesmo T (o próprio `docker run -d` leva segundos): m1 (a geração
# longa) começa em T, o batch em T+1 s e o interactive em T+2 s.
BASE=$(( $(date +%s) + 25 ))
run_timed() { # run_timed <nome> <chave> <max_tokens> <segundos depois de BASE>
  local at=$(( BASE + $4 ))
  MSYS_NO_PATHCONV=1 docker run -d --name "aic-curl-$1" --network "$NET" -v aic-order-m:/o "$IMG" sh -c     "while [ \$(date +%s) -lt $at ]; do sleep 0.05; done; date +%s%N > /o/$1.start;       curl -s -o /dev/null -H 'Authorization: Bearer $2' -H 'Content-Type: application/json' --max-time 90       -d '{\"model\":\"gpt2\",\"max_tokens\":$3,\"messages\":[{\"role\":\"user\",\"content\":\"The capital of France is\"}]}'       $GATEWAY/v1/chat/completions; date +%s%N > /o/$1" >/dev/null
}
run_timed m1 kg 64 0
run_timed m2 kl 3 1
run_timed m3 kh 3 2
for _ in $(seq 1 40); do
  [ "$(docker ps -q -f name=aic-curl-m | wc -l)" = 0 ] && break
  sleep 2
done
LO=$(MSYS_NO_PATHCONV=1 docker run --rm -v aic-order-m:/o "$IMG" cat /o/m2)
HI=$(MSYS_NO_PATHCONV=1 docker run --rm -v aic-order-m:/o "$IMG" cat /o/m3)
for n in m1 m2 m3; do
  echo "$n: início $(MSYS_NO_PATHCONV=1 docker run --rm -v aic-order-m:/o "$IMG" cat /o/$n.start)"
done
M1=$(MSYS_NO_PATHCONV=1 docker run --rm -v aic-order-m:/o "$IMG" cat /o/m1)
echo "m1 fim: $M1"
echo "batch terminou em $LO; interactive terminou em $HI"
[ "$HI" -lt "$LO" ] || fail "o interactive (chegou depois) deveria terminar antes do batch"
echo "OK: o interactive passou na frente do batch"

echo "=== D) grafo abandonado é cancelado no controller ==="
# Sem nenhum worker o grafo nunca roda; o gateway desiste no prazo e manda cancelar.
CODE=$(http -o /dev/null -w '%{http_code}' --max-time 60 -H "Authorization: Bearer ka" -H "Content-Type: application/json" \
  -d '{"model":"matmul-diamond-v0","input":"oi"}' $GATEWAY/v1/embeddings)
echo "(acme não tem embeddings; usando a chave beta)"
CODE=$(http -o /dev/null -w '%{http_code}' --max-time 60 -H "Authorization: Bearer kb" -H "Content-Type: application/json" \
  -d '{"model":"matmul-diamond-v0","input":"oi"}' $GATEWAY/v1/embeddings)
[ "$CODE" = 504 ] || fail "embeddings sem worker deveriam dar 504, deu $CODE"
MSYS_NO_PATHCONV=1 docker run --rm "${common[@]:0:4}" -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_OPERATOR_TOKEN=$OPERATOR \
  -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$R/ai-cluster events --limit 50" | grep request_cancelled \
  || fail "faltou o evento request_cancelled"
echo "OK: 504 e request_cancelled"

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== fim ==="
