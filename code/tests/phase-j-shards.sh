#!/usr/bin/env bash
# Fatia da Fase J (J5): as projeções da SmolLM2 rodam em workers com fatias residentes.
#
# Critério verificado: com 2 workers, cada um com metade das linhas de cada projeção carregada do
# próprio disco, o gateway devolve a mesma continuação que o modelo local; os pesos nunca
# trafegam (o gateway não os usa); sem um dos workers o pedido falha com 503 em vez de pendurar.
# Imprime o tempo por chamada que o gateway mediu (rede + relay + cálculo).
#
# Uso: phase-j-shards.sh [smollm2|gpt2]. O GPT-2 fatia por colunas (Conv1D `[in, out]`), a
# SmolLM2 por linhas (`[out, in]`).
#
# Rode a partir de code/, com code/models/<modelo> já baixado.
set -uo pipefail

case "${1:-smollm2}" in
  smollm2) ID=smollm2-135m; DIR=smollm2; VAR=SMOLLM2; EXPECT=' the capital of the country' ;;
  gpt2) ID=gpt2; DIR=gpt2; VAR=GPT2; EXPECT=' the capital of the French' ;;
  *) echo "uso: $0 [smollm2|gpt2]"; exit 1 ;;
esac

NET=aic-net
IMG=aic-rust:1
R=/t/release
JOIN=h-token
OPERATOR=h-operator
KEY=h-api-key
CONTROLLER=http://aic-controller:50051
GATEWAY=http://aic-gateway:8080

[ -f "models/$DIR/model.safetensors" ] || { echo "FALHA: faltam os pesos em code/models/$DIR"; exit 1; }

teardown() {
  docker rm -f aic-controller aic-worker-1 aic-worker-2 aic-gateway >/dev/null 2>&1
}

fail() {
  echo "FALHA: $1"
  docker logs aic-gateway 2>&1 | tail -8
  docker logs aic-worker-1 2>&1 | tail -3
  teardown
  exit 1
}

common=(--network "$NET" -v aic-target:/t -e AI_CLUSTER_INSECURE=1
  -e AI_CLUSTER_JOIN_TOKEN=$JOIN -e AI_CLUSTER_OPERATOR_TOKEN=$OPERATOR)

http() {
  MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" "$IMG" curl -s "$@"
}

chat() { # chat <max_tokens> -> resposta
  http -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" --max-time 120 \
    -d "{\"model\":\"$ID\",\"max_tokens\":$1,\"messages\":[{\"role\":\"user\",\"content\":\"The capital of France is\"}]}" \
    $GATEWAY/v1/chat/completions
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

MSYS_NO_PATHCONV=1 docker run -d --name aic-controller "${common[@]}" -e AI_CLUSTER_AUTO_APPROVE=1 \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
sleep 2
for i in 1 2; do
  MSYS_NO_PATHCONV=1 docker run -d --name "aic-worker-$i" "${common[@]}" \
    -v "$PWD/models:/models:ro" -e AI_CLUSTER_MODEL_DIR=/models \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID="worker-$i" \
    "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
done
sleep 6
for i in 1 2; do
  docker logs "aic-worker-$i" 2>&1 | grep -q "canal de fatias aberto" || fail "worker-$i não abriu o canal"
done
echo "OK: os dois workers abriram o canal residente"

# O gateway só lê config, formas e tensores pequenos; as projeções ficam nos workers.
MSYS_NO_PATHCONV=1 docker run -d --name aic-gateway "${common[@]}" -e AI_CLUSTER_API_KEY=$KEY \
  -v "$PWD/models/$DIR:/model:ro" -e AI_CLUSTER_${VAR}_DIR=/model \
  -e AI_CLUSTER_${VAR}_SHARDS=worker-1,worker-2 -e AI_CLUSTER_${VAR}_SUBDIR=$DIR \
  -e AI_CLUSTER_GATEWAY_ADDR=0.0.0.0:8080 -e AI_CLUSTER_CONTROLLER=$CONTROLLER \
  "$IMG" sh -c "exec $R/ai-cluster-gateway" >/dev/null
for _ in $(seq 1 60); do
  http $GATEWAY/health | grep -q green && break
  sleep 2
done
http $GATEWAY/health | grep -q green || fail "o gateway não subiu"

echo "=== A) a primeira chamada carrega as fatias; a segunda já as encontra residentes ==="
FIRST=$(chat 5)
echo "$FIRST"
echo "$FIRST" | grep -q "\"content\":\"$EXPECT\"" \
  || fail "a continuação com fatias deveria ser a mesma do modelo local"
SECOND=$(chat 5)
echo "$SECOND" | grep -q "\"content\":\"$EXPECT\"" || fail "segunda chamada divergiu"
docker logs aic-gateway 2>&1 | grep "remoto:" | tail -2
echo "OK: a continuação com as projeções nos workers é idêntica à local"

echo "=== B) um worker morre no meio da geração: as faixas dele vão para o outro ==="
OUT=$(mktemp)
chat 15 >"$OUT" &
sleep 1
docker rm -f aic-worker-2 >/dev/null 2>&1
wait
cat "$OUT"; echo
grep -q "\"content\":\"$EXPECT" "$OUT" || { rm -f "$OUT"; fail "a geração deveria seguir igual com um worker só"; }
rm -f "$OUT"
docker logs aic-gateway 2>&1 | grep -q "refazendo nos demais" || fail "o gateway deveria ter refeito a chamada"
echo "OK: a geração terminou com a continuação idêntica depois de perder worker-2"

echo "=== C) o último worker some: o pedido falha rápido, sem pendurar ==="
docker rm -f aic-worker-1 >/dev/null 2>&1
sleep 2
CODE=$(http -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" --max-time 120 \
  -d "{\"model\":\"$ID\",\"max_tokens\":3,\"messages\":[{\"role\":\"user\",\"content\":\"Hello\"}]}" \
  $GATEWAY/v1/chat/completions)
[ "$CODE" = 503 ] || fail "sem nenhum worker o pedido deveria dar 503, deu: $CODE"
echo "OK: 503 sem nenhum worker"

teardown
echo "=== J5 canal residente OK ==="
