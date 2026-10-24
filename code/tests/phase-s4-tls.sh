#!/usr/bin/env bash
# Fatia da Fase S (S4): TLS no canal gRPC.
#
# Critério verificado: com certificado, worker e CLI falam com o controller por https verificando
# uma CA; http puro, CA errada e endpoint http sem AI_CLUSTER_INSECURE são recusados, e um
# controller sem certificado não sobe a menos que se opte por AI_CLUSTER_INSECURE=1.
#
# Sequência:
#   A) controller sem certificado nem INSECURE recusa subir.
#   B) controller com certificado: worker https com a CA certa fica READY.
#   C) worker http (com INSECURE) contra o controller TLS é recusado por erro de transporte.
#   D) worker https com a CA errada é recusado por erro de certificado.
#   E) worker http sem INSECURE nem sequer tenta: o cliente recusa o endpoint.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
TOKEN=s4-token
OPERATOR=s4-operator
HOST=aic-controller

teardown() {
  docker rm -f aic-controller aic-w-ok aic-w-http aic-w-wrongca aic-w-noinsecure >/dev/null 2>&1
}

fail() {
  echo "FALHA: $1"
  teardown
  exit 1
}

# Os certificados vivem num volume: uma CA certa, uma CA que não assinou nada e o certificado do
# controller com SAN=aic-controller.
docker volume rm aic-tls >/dev/null 2>&1
MSYS_NO_PATHCONV=1 docker run --rm -v aic-tls:/tls "$IMG" sh -c '
  set -e; cd /tls
  openssl req -x509 -newkey rsa:2048 -nodes -keyout ca.key -out ca.pem -days 2 -subj /CN=aic-ca 2>/dev/null
  openssl req -x509 -newkey rsa:2048 -nodes -keyout other.key -out other.pem -days 2 -subj /CN=other-ca 2>/dev/null
  openssl req -newkey rsa:2048 -nodes -keyout srv.key -out srv.csr -subj /CN=aic-controller 2>/dev/null
  printf "subjectAltName=DNS:aic-controller\n" > san.cnf
  openssl x509 -req -in srv.csr -CA ca.pem -CAkey ca.key -CAcreateserial -out srv.pem -days 2 -extfile san.cnf 2>/dev/null
  chmod 644 *.pem *.key' || fail "não consegui gerar os certificados"

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

# worker <nome> <nó> <endpoint> <ca|-> <insecure 0|1>
worker() {
  local env=()
  [ "$4" != "-" ] && env+=(-e "AI_CLUSTER_TLS_CA=$4")
  [ "$5" = 1 ] && env+=(-e AI_CLUSTER_INSECURE=1)
  MSYS_NO_PATHCONV=1 docker run -d --name "$1" --network "$NET" -v aic-target:/t -v aic-tls:/tls "${env[@]}" \
    -e AI_CLUSTER_JOIN_TOKEN=$TOKEN -e AI_CLUSTER_CONTROLLER="$3" -e AI_CLUSTER_NODE_ID="$2" \
    "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
}

cli() {
  MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" -v aic-target:/t -v aic-tls:/tls \
    -e AI_CLUSTER_OPERATOR_TOKEN=$OPERATOR -e AI_CLUSTER_TLS_CA=/tls/ca.pem \
    -e AI_CLUSTER_CONTROLLER=https://$HOST:50051 "$IMG" sh -c "$R/ai-cluster $*" 2>&1
}

echo "=== A) controller sem certificado nem INSECURE recusa subir ==="
OUT=$(MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_JOIN_TOKEN=$TOKEN -e AI_CLUSTER_OPERATOR_TOKEN=$OPERATOR \
  "$IMG" sh -c "$R/ai-cluster-controller" 2>&1)
if echo "$OUT" | grep -q "AI_CLUSTER_TLS_CERT"; then
  echo "OK: o controller sem TLS não subiu"
else
  fail "o controller deveria recusar subir sem certificado, saiu: $OUT"
fi

# AI_CLUSTER_INSECURE junto com o certificado: o certificado vence, o opt-out esquecido não rebaixa.
MSYS_NO_PATHCONV=1 docker run -d --name aic-controller --network "$NET" -v aic-target:/t -v aic-tls:/tls \
  -e AI_CLUSTER_JOIN_TOKEN=$TOKEN -e AI_CLUSTER_OPERATOR_TOKEN=$OPERATOR -e AI_CLUSTER_AUTO_APPROVE=1 \
  -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_TLS_CERT=/tls/srv.pem -e AI_CLUSTER_TLS_KEY=/tls/srv.key \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
sleep 3

echo "=== B) https com a CA certa entra ==="
worker aic-w-ok node-ok https://$HOST:50051 /tls/ca.pem 0
sleep 8
NODES=$(cli nodes)
echo "$NODES"
if echo "$NODES" | grep -E "^node-ok " | grep -q READY; then
  echo "OK: node-ok está READY por https"
else
  docker logs aic-w-ok 2>&1 | tail -3
  fail "node-ok deveria estar READY por https"
fi

# refused <container> <nó> <rótulo> <erro esperado>: não entrou, e o log traz um erro de
# transporte/certificado (não de token, nem outra falha qualquer do worker).
refused() {
  sleep 8
  if cli nodes | grep -q "$2"; then
    fail "$3: o nó $2 não deveria ter entrado"
  fi
  local log
  log=$(docker logs "$1" 2>&1)
  if echo "$log" | grep -qi "unauthenticated"; then
    echo "$log" | tail -3
    fail "$3: a recusa deveria vir do transporte, não do token"
  fi
  if ! echo "$log" | grep -qiE "$4"; then
    echo "$log" | tail -3
    fail "$3: o log deveria trazer $4"
  fi
  echo "$log" | tail -2
  echo "OK: $3"
}

echo "=== C) http puro contra o controller TLS ==="
worker aic-w-http node-http http://$HOST:50051 - 1
refused aic-w-http node-http "http puro foi recusado" "transport"

echo "=== D) https com a CA errada ==="
worker aic-w-wrongca node-wrongca https://$HOST:50051 /tls/other.pem 0
refused aic-w-wrongca node-wrongca "a CA errada foi recusada" "InvalidCertificate"

echo "=== E) http sem INSECURE: o cliente recusa o endpoint ==="
worker aic-w-noinsecure node-noinsecure http://$HOST:50051 - 0
sleep 4
if docker logs aic-w-noinsecure 2>&1 | grep -q "AI_CLUSTER_INSECURE"; then
  echo "OK: o cliente recusou o endpoint sem TLS"
else
  docker logs aic-w-noinsecure 2>&1 | tail -3
  fail "o cliente deveria recusar http sem AI_CLUSTER_INSECURE"
fi

teardown
echo "=== S4 TLS OK ==="
