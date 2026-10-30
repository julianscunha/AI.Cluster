#!/usr/bin/env bash
# Fatia da F19: mTLS no canal gRPC.
#
# Critério verificado: com AI_CLUSTER_TLS_CLIENT_CA, o controller só fala com quem apresenta um
# certificado de cliente assinado por essa CA. A recusa vem do handshake, antes de qualquer token.
#
# Sequência:
#   A) controller com CLIENT_CA mas sem certificado de servidor recusa subir.
#   B) worker com certificado de cliente da CA certa fica READY, e a CLI com o dela também.
#   C) worker sem certificado de cliente é recusado no transporte.
#   D) worker com certificado de cliente de outra CA é recusado no transporte.
#   E) certificado de cliente sem a chave (metade do par) é recusado pelo cliente.
set -uo pipefail

NET=aic-net-mtls
IMG=aic-rust:1
R=/t/release
TOKEN=mtls-token
OPERATOR=mtls-operator
HOST=aic-controller-mtls

teardown() {
  docker rm -f $HOST aic-w-mtls-ok aic-w-mtls-none aic-w-mtls-other aic-w-mtls-half >/dev/null 2>&1
  docker network rm "$NET" >/dev/null 2>&1
}

fail() {
  echo "FALHA: $1"
  teardown
  exit 1
}

# CA do servidor, CA de clientes, uma CA estranha, e os certificados de cada um.
docker volume rm aic-tls-mtls >/dev/null 2>&1
MSYS_NO_PATHCONV=1 docker run --rm -v aic-tls-mtls:/tls "$IMG" sh -c '
  set -e; cd /tls
  mkca() { openssl req -x509 -newkey rsa:2048 -nodes -keyout $1.key -out $1.pem -days 2 -subj /CN=$1 2>/dev/null; }
  mkca ca; mkca clients; mkca stranger
  openssl req -newkey rsa:2048 -nodes -keyout srv.key -out srv.csr -subj /CN=aic-controller-mtls 2>/dev/null
  printf "subjectAltName=DNS:aic-controller-mtls\n" > san.cnf
  openssl x509 -req -in srv.csr -CA ca.pem -CAkey ca.key -CAcreateserial -out srv.pem -days 2 -extfile san.cnf 2>/dev/null
  printf "extendedKeyUsage=clientAuth
" > cli.cnf
  for who in ok:clients other:stranger; do
    name=${who%%:*}; issuer=${who##*:}
    openssl req -newkey rsa:2048 -nodes -keyout $name.key -out $name.csr -subj /CN=node-$name 2>/dev/null
    openssl x509 -req -in $name.csr -CA $issuer.pem -CAkey $issuer.key -CAcreateserial -out $name.pem -days 2 -extfile cli.cnf 2>/dev/null
  done
  chmod 644 *.pem *.key' || fail "não consegui gerar os certificados"

teardown
docker network create "$NET" >/dev/null

echo "=== A) CLIENT_CA sem certificado de servidor não sobe ==="
OUT=$(MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" -v aic-target:/t -v aic-tls-mtls:/tls \
  -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=$TOKEN -e AI_CLUSTER_OPERATOR_TOKEN=$OPERATOR \
  -e AI_CLUSTER_TLS_CLIENT_CA=/tls/clients.pem "$IMG" sh -c "$R/ai-cluster-controller" 2>&1)
echo "$OUT" | grep -q "AI_CLUSTER_TLS_CLIENT_CA exige" && echo "OK: recusou subir" \
  || fail "deveria recusar subir só com CLIENT_CA, saiu: $OUT"

MSYS_NO_PATHCONV=1 docker run -d --name $HOST --network "$NET" -v aic-target:/t -v aic-tls-mtls:/tls \
  -e AI_CLUSTER_JOIN_TOKEN=$TOKEN -e AI_CLUSTER_OPERATOR_TOKEN=$OPERATOR -e AI_CLUSTER_AUTO_APPROVE=1 \
  -e AI_CLUSTER_TLS_CERT=/tls/srv.pem -e AI_CLUSTER_TLS_KEY=/tls/srv.key \
  -e AI_CLUSTER_TLS_CLIENT_CA=/tls/clients.pem \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
sleep 3

# worker <nome> <nó> [certificado de cliente] [chave de cliente]
worker() {
  local env=()
  [ -n "${3:-}" ] && env+=(-e "AI_CLUSTER_TLS_CLIENT_CERT=$3")
  [ -n "${4:-}" ] && env+=(-e "AI_CLUSTER_TLS_CLIENT_KEY=$4")
  MSYS_NO_PATHCONV=1 docker run -d --name "$1" --network "$NET" -v aic-target:/t -v aic-tls-mtls:/tls "${env[@]}" \
    -e AI_CLUSTER_TLS_CA=/tls/ca.pem -e AI_CLUSTER_JOIN_TOKEN=$TOKEN \
    -e AI_CLUSTER_CONTROLLER=https://$HOST:50051 -e AI_CLUSTER_NODE_ID="$2" \
    "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
}

cli() {
  MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" -v aic-target:/t -v aic-tls-mtls:/tls \
    -e AI_CLUSTER_OPERATOR_TOKEN=$OPERATOR -e AI_CLUSTER_TLS_CA=/tls/ca.pem "$@" \
    -e AI_CLUSTER_CONTROLLER=https://$HOST:50051 "$IMG" sh -c "$R/aic cluster nodes" 2>&1
}

echo "=== B) certificado de cliente da CA certa entra ==="
worker aic-w-mtls-ok node-ok /tls/ok.pem /tls/ok.key
sleep 8
NODES=$(cli -e AI_CLUSTER_TLS_CLIENT_CERT=/tls/ok.pem -e AI_CLUSTER_TLS_CLIENT_KEY=/tls/ok.key)
echo "$NODES"
echo "$NODES" | grep -E "^node-ok " | grep -q READY && echo "OK: node-ok READY e a CLI com certificado enxerga" \
  || { docker logs aic-w-mtls-ok 2>&1 | tail -3; fail "node-ok deveria estar READY"; }

# refused <container> <nó> <rótulo> <erro esperado>
refused() {
  sleep 8
  if cli -e AI_CLUSTER_TLS_CLIENT_CERT=/tls/ok.pem -e AI_CLUSTER_TLS_CLIENT_KEY=/tls/ok.key | grep -q "$2"; then
    fail "$3: o nó $2 não deveria ter entrado"
  fi
  local log
  log=$(docker logs "$1" 2>&1)
  echo "$log" | grep -qi "unauthenticated" && { echo "$log" | tail -3; fail "$3: a recusa deveria vir do transporte, não do token"; }
  echo "$log" | grep -qiE "$4" || { echo "$log" | tail -3; fail "$3: o log deveria trazer $4"; }
  echo "$log" | tail -2
  echo "OK: $3"
}

echo "=== C) sem certificado de cliente ==="
worker aic-w-mtls-none node-none
refused aic-w-mtls-none node-none "sem certificado de cliente foi recusado" "CertificateRequired"

echo "=== D) certificado de cliente de outra CA ==="
worker aic-w-mtls-other node-other /tls/other.pem /tls/other.key
refused aic-w-mtls-other node-other "certificado de outra CA foi recusado" "UnknownCA"

echo "=== E) metade do par ==="
worker aic-w-mtls-half node-half /tls/ok.pem
sleep 4
docker logs aic-w-mtls-half 2>&1 | grep -q "vêm juntos" && echo "OK: o cliente recusou o par incompleto" \
  || { docker logs aic-w-mtls-half 2>&1 | tail -3; fail "o cliente deveria recusar o par incompleto"; }

teardown
echo "=== mTLS OK ==="
