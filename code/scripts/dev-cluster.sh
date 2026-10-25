#!/usr/bin/env bash
# Cluster de desenvolvimento em contêineres, para acompanhar com a CLI.
#
#   bash code/scripts/dev-cluster.sh build        compila os binários no volume aic-target
#   bash code/scripts/dev-cluster.sh up [N]       sobe o controller e N workers (padrão 2)
#   bash code/scripts/dev-cluster.sh cli <args>   roda `ai-cluster <args>` contra esse cluster
#   bash code/scripts/dev-cluster.sh metrics      lê o /metrics do Prometheus (com o token)
#   bash code/scripts/dev-cluster.sh down         derruba tudo
#
# Exemplos:  ... cli dashboard --watch 2   |   ... cli benchmark --size 256 --repetitions 8
#
# Sem TLS (AI_CLUSTER_INSECURE=1) e com aprovação automática: é para a máquina de
# desenvolvimento, não para uma rede aberta. Os tokens são os de abaixo.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
JOIN=dev-token
OPERATOR=dev-operator
CODE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export MSYS_NO_PATHCONV=1

common=(--network "$NET" -v aic-target:/t -e AI_CLUSTER_INSECURE=1
  -e AI_CLUSTER_JOIN_TOKEN=$JOIN -e AI_CLUSTER_OPERATOR_TOKEN=$OPERATOR)

case "${1:-}" in
build)
  docker run --rm -v "$CODE:/w" -v aic-target:/t -e CARGO_TARGET_DIR=/t -w /w "$IMG" \
    cargo build --release --workspace
  ;;
up)
  workers=${2:-2}
  docker network create "$NET" >/dev/null 2>&1
  docker rm -f aic-controller >/dev/null 2>&1
  docker run -d --name aic-controller "${common[@]}" -e AI_CLUSTER_AUTO_APPROVE=1 -e AI_CLUSTER_METRICS_ADDR=0.0.0.0:9101 \
    -p 9101:9101 "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null || exit 1
  for i in $(seq 1 "$workers"); do
    docker rm -f "aic-worker-$i" >/dev/null 2>&1
    docker run -d --name "aic-worker-$i" "${common[@]}" \
      -e AI_CLUSTER_CONTROLLER=http://aic-controller:50051 -e AI_CLUSTER_NODE_ID="worker-$i" \
      "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
  done
  echo "controller e $workers workers no ar. Tente:"
  echo "  bash $0 cli nodes"
  echo "  bash $0 cli dashboard --watch 2"
  echo "  bash $0 cli benchmark --size 256 --repetitions 8"
  ;;
cli)
  shift
  docker run --rm "${common[@]}" -e AI_CLUSTER_CONTROLLER=http://aic-controller:50051 \
    "$IMG" sh -c "$R/ai-cluster $*"
  ;;
metrics)
  docker run --rm "${common[@]}" "$IMG" \
    curl -s -H "Authorization: Bearer $OPERATOR" http://aic-controller:9101/metrics
  ;;
down)
  docker ps -aq --filter name='aic-worker-' | xargs -r docker rm -f >/dev/null
  docker rm -f aic-controller >/dev/null 2>&1
  echo "derrubado"
  ;;
*)
  sed -n 2,13p "$0"
  ;;
esac
