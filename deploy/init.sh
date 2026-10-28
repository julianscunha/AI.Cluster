#!/usr/bin/env bash
# Prepara um cluster de teste em um computador só: tokens aleatórios, a CA do túnel RPC e os
# certificados do gateway e dos workers, em deploy/data/ (fora do git). Rode uma vez.
#
#   bash deploy/init.sh && docker compose -f deploy/docker-compose.yml up -d --build
#
# Precisa de bash e openssl. Os segredos ficam em deploy/data/*.env e deploy/data/pki.
set -euo pipefail
export MSYS_NO_PATHCONV=1  # o Git Bash do Windows trocaria "/CN=..." por um caminho
cd "$(dirname "$0")"
DATA=data
if [ -f "$DATA/controller.env" ]; then
  echo "deploy/data já existe: nada a fazer (apague a pasta para recomeçar)"
  exit 0
fi
mkdir -p "$DATA/pki" models
rand() { openssl rand -hex 24; }
umask 077
JOIN=$(rand)
OPERATOR=$(rand)
INFERENCE=$(rand)
API_KEY="aic_env_$(rand)"
# Um arquivo por papel: o worker só conhece o token de join, e o gateway nunca recebe o de operador.
{
  echo "AI_CLUSTER_JOIN_TOKEN=$JOIN"
  echo "AI_CLUSTER_OPERATOR_TOKEN=$OPERATOR"
  echo "AI_CLUSTER_INFERENCE_TOKEN=$INFERENCE"
} > "$DATA/controller.env"
{
  echo "AI_CLUSTER_INFERENCE_TOKEN=$INFERENCE"
  echo "AI_CLUSTER_API_KEY=$API_KEY"
} > "$DATA/gateway.env"
echo "AI_CLUSTER_JOIN_TOKEN=$JOIN" > "$DATA/worker.env"
cd "$DATA/pki"
# Uma CA que só assina o gateway e os workers deste cluster (30 dias; renove apagando deploy/data).
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout ca.key -out ca.crt \
  -subj "/CN=ai-cluster-ca" -days 30 -addext "basicConstraints=critical,CA:TRUE" \
  -addext "keyUsage=critical,keyCertSign,cRLSign" >/dev/null 2>&1
issue() { # nome  SAN
  openssl req -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout "$1.key" -out "$1.csr" \
    -subj "/CN=$1" >/dev/null 2>&1
  printf 'subjectAltName=DNS:%s\nbasicConstraints=CA:FALSE\n' "$2" > "$1.ext"
  openssl x509 -req -in "$1.csr" -CA ca.crt -CAkey ca.key -CAcreateserial -out "$1.crt" -days 30 \
    -extfile "$1.ext" >/dev/null 2>&1
  rm -f "$1.csr" "$1.ext"
}
issue worker aic-worker
issue gateway aic-gateway
# Os contêineres rodam como usuário sem privilégio e precisam ler as chaves. A chave da CA não
# precisa estar em lugar nenhum além daqui.
chmod 644 worker.key worker.crt gateway.key gateway.crt ca.crt
chmod 600 ca.key
echo "pronto: segredos em deploy/data/*.env e certificados em deploy/data/pki"
echo "ponha modelos GGUF em deploy/models e suba: docker compose -f deploy/docker-compose.yml up -d --build"
