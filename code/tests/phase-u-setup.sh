#!/usr/bin/env bash
# Instalador (aic-setup): servidor + convite de um worker, em dois contêineres Debian 12 sem systemd.
#
#   bash code/scripts/package-cluster.sh ai-cluster:local dist && bash code/tests/phase-u-setup.sh [dist]
#
# Passa se `aic-setup server` sobe controller e gateway, e o worker que roda a linha do convite entra
# e é aprovado sozinho (READY e "aprovado" no `ai-cluster nodes`).
set -uo pipefail
export MSYS_NO_PATHCONV=1
DIST=$(cd "${1:-dist}" && pwd -W 2>/dev/null || pwd)
NET=aic-st
teardown() { docker rm -f aic-srv aic-wk >/dev/null 2>&1; docker network rm $NET >/dev/null 2>&1; }
fail() { echo "FALHA: $1"; docker exec aic-srv tail -5 /tmp/invite.log 2>&1; teardown; exit 1; }
trap teardown EXIT
teardown; docker network create $NET >/dev/null
DEPS="apt-get update -qq && apt-get install -y -qq openssl libgomp1 iproute2 python3 curl >/dev/null 2>&1"
for n in aic-srv aic-wk; do docker run -d --name $n --network $NET -v "$DIST:/dist" debian:12 sleep 900 >/dev/null; done
docker exec aic-srv sh -c "$DEPS" & docker exec aic-wk sh -c "$DEPS" & wait
docker exec aic-srv sh -c "cd /tmp && tar xzf /dist/ai-cluster-x86_64.tar.gz && cd ai-cluster-x86_64 && ./aic-setup server --yes" \
  || fail "aic-setup server"
docker exec -d aic-srv sh -c "aic-setup invite --minutes 2 > /tmp/invite.log 2>&1"; sleep 4
docker exec aic-wk sh -c "curl -fsSL http://\$(getent hosts aic-srv | cut -d' ' -f1):8099/join | sh" || fail "o worker não entrou"
for _ in $(seq 1 20); do
  docker exec aic-srv ai-cluster nodes 2>/dev/null | grep -E "READY.*aprovado" >/dev/null && { echo "=== instalador OK ==="; exit 0; }
  sleep 2
done
fail "o nó não ficou READY e aprovado"
