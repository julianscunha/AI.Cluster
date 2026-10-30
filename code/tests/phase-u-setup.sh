#!/usr/bin/env bash
# Instalador guiado (install.sh + aic-setup): servidor, worker e gateway noutro computador, em
# contêineres Debian 12 sem systemd, com um espelho local no lugar da release do GitHub.
#
#   bash code/scripts/package-release.sh dist/release && bash code/tests/phase-u-setup.sh [dist/release]
#
# Critérios verificados:
#   A) install.sh instala o servidor (controller + gateway) baixando só os pacotes desse papel;
#   B) o convite entrega o pacote do worker pelo processador e o nó fica READY e aprovado;
#   B2) o /wait do convite apresenta um computador com Linux para instalação e o operador o vê;
#   C) o controller sozinho e o gateway em outro computador sobem (o gateway com o controller dele);
#   D) processador errado, gateway em ARM de 32 bits e ARM de 64 bits são recusados com mensagem.
set -uo pipefail
export MSYS_NO_PATHCONV=1
DIST=$(cd "${1:-dist/release}" && pwd -W 2>/dev/null || pwd)
NET=aic-st
BASE=http://aic-mirror:9000
teardown() { docker rm -f aic-mirror aic-srv aic-wk aic-ctl aic-gw >/dev/null 2>&1; docker network rm $NET >/dev/null 2>&1; }
fail() { echo "FALHA: $1"; docker exec aic-srv tail -5 /tmp/invite.log 2>&1; teardown; exit 1; }
trap teardown EXIT
teardown; docker network create $NET >/dev/null
# Imagem base com as dependências do instalador (construída uma vez; o cache do Docker a reaproveita).
docker build -q -t aic-test-base - >/dev/null <<'DOCKERFILE' || { echo "FALHA: imagem base"; exit 1; }
FROM debian:12
RUN apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq openssl libgomp1 iproute2 python3 curl procps >/dev/null && rm -rf /var/lib/apt/lists/*
DOCKERFILE
docker run -d --name aic-mirror --network $NET -v "$DIST:/r:ro" -w /r aic-test-base python3 -m http.server 9000 >/dev/null
for n in aic-srv aic-wk aic-ctl aic-gw; do docker run -d --name $n --network $NET aic-test-base sleep 900 >/dev/null; done
for n in aic-srv aic-wk aic-ctl aic-gw; do
  for _ in $(seq 60); do docker exec $n curl -fsS $BASE/install.sh -o /tmp/install.sh 2>/dev/null && break; sleep 2; done
  docker exec $n test -s /tmp/install.sh || fail "sem install.sh no espelho"
done
inst() { n=$1; shift; docker exec $n sh -c "AIC_BASE=$BASE sh /tmp/install.sh --mode linux --kind conteiner --yes $*" 2>&1; }

echo "=== A) servidor completo baixa só controller, gateway e tools ==="
OUT=$(inst aic-srv --role server --arch x86_64) || { echo "$OUT"; fail "install.sh server"; }
echo "$OUT" | grep -q "✔ gateway-linux-x86_64" || fail "o gateway deveria ter sido baixado"
echo "$OUT" | grep -q "worker-linux" && fail "o servidor não deveria baixar o worker"
echo "OK: servidor instalado"

echo "=== B) o convite entrega o worker e o nó fica READY e aprovado ==="
docker exec -d aic-srv sh -c "aic-setup invite --minutes 2 > /tmp/invite.log 2>&1"; sleep 25
SIP=$(docker exec aic-srv cat /etc/ai-cluster/server-ip)
inst aic-wk --role worker --arch x86_64 --server "$SIP" >/dev/null || fail "o worker não entrou"
for _ in $(seq 20); do
  docker exec aic-srv ai-cluster nodes 2>/dev/null | grep -E "READY.*aprovado" >/dev/null && break; sleep 2
done
docker exec aic-srv ai-cluster nodes 2>/dev/null | grep -E "READY.*aprovado" >/dev/null || fail "o nó não ficou READY e aprovado"
echo "OK: worker READY e aprovado"
# Certificado por nó: a chave nasceu no worker (o servidor não tem chave de worker) e o SAN leva o nome do nó.
docker exec aic-srv test ! -e /etc/ai-cluster/pki/worker.key || fail "o servidor não deveria guardar chave de worker"
WNAME=$(docker exec aic-wk hostname | sed 's/[^A-Za-z0-9.-]/-/g' | cut -c1-40)
docker exec aic-wk sh -c "openssl x509 -in /etc/ai-cluster/rpc-tls/worker.crt -noout -ext subjectAltName | grep -q 'DNS:aic-worker-$WNAME' && openssl verify -CAfile /etc/ai-cluster/rpc-tls/ca.crt /etc/ai-cluster/rpc-tls/worker.crt >/dev/null"   || fail "o certificado do worker deveria ter o SAN aic-worker-$WNAME e conferir com a CA"
docker exec aic-srv grep -q 'AI_CLUSTER_LLAMA_RPC_SERVER_NAME=aic-worker-{node_id}' /etc/ai-cluster/gateway.env || fail "o gateway deveria esperar o nome por nó"
echo "OK: certificado por nó"

echo "=== B2) quem já tem Linux se apresenta pelo /wait e aparece para aprovar ==="
# O convite que está aberto também abriu a porta de espera; o script baixa só o aic-wait e o roda.
docker exec aic-wk sh -c "curl -fsSL http://$SIP:8099/wait -o /tmp/wait.sh" || fail "o servidor não serviu o /wait"
docker exec -d aic-wk sh -c "timeout 40 sh /tmp/wait.sh > /tmp/wait.log 2>&1"
for _ in $(seq 20); do
  docker exec aic-srv ai-cluster installs 2>/dev/null | grep -q "fingerprint=.* aguardando" && break; sleep 2
done
docker exec aic-srv ai-cluster installs | grep -q "fingerprint=.* aguardando" || fail "o aparelho não apareceu em installs: $(docker exec aic-wk tail -5 /tmp/wait.log)"
docker exec aic-wk grep -q "Impressão digital" /tmp/wait.log || fail "a tela de espera não mostrou a impressão digital"
for _ in $(seq 10); do docker exec aic-srv grep -q "quer instalar" /tmp/invite.log && break; sleep 2; done
docker exec aic-srv grep -q "quer instalar" /tmp/invite.log || fail "o convite deveria avisar o operador do aparelho"
echo "OK: aparelho aguardando aprovação"

echo "=== C) controller sozinho e gateway noutro computador ==="
inst aic-ctl --role controller --arch x86_64 >/dev/null || fail "install.sh controller"
docker exec -d aic-ctl sh -c "aic-setup invite --minutes 2 > /tmp/invite.log 2>&1"; sleep 25
CIP=$(docker exec aic-ctl cat /etc/ai-cluster/server-ip)
GCODE=$(docker exec aic-ctl sh -c "grep -o -- '--code [A-Za-z0-9]*' /tmp/invite.log | head -n 1 | cut -d' ' -f2")
[ -n "$GCODE" ] || fail "o convite deveria mostrar o código do gateway"
inst aic-gw --role gateway --arch x86_64 --server "$CIP" >/dev/null 2>&1 && fail "o gateway sem código deveria ser recusado"
docker exec aic-gw sh -c "AIC_CODE=$GCODE AIC_BASE=$BASE sh /tmp/install.sh --mode linux --kind conteiner --yes --role gateway --arch x86_64 --server $CIP" >/dev/null 2>&1 || fail "install.sh gateway"
docker exec aic-gw sh -c "ss -ltn | grep -q ':8080 '" || fail "o gateway não abriu a porta 8080"
docker exec aic-gw grep -q "controller http://$CIP:50051" /var/log/ai-cluster/gateway.log || fail "o gateway deveria apontar para o controller"
echo "OK: controller e gateway em computadores separados"

echo "=== D) combinações recusadas ==="
# Capturar a saída antes do grep: `grep -q` fecha o pipe cedo e o pipefail contaria como falha.
refused() { out=$(inst aic-ctl "$@"); case "$out" in *"$MSG"*) ;; *) fail "esperava \"$MSG\" para: $*" ;; esac; }
MSG="só existe para x86_64" refused --role server --arch armv7
MSG="ARM de 64 bits ainda não" refused --role worker --arch aarch64 --server 1.2.3.4
MSG="este computador é x86_64" refused --role worker --arch armv7 --server 1.2.3.4
echo "OK: recusas com mensagem"
echo "=== instalador guiado OK ==="
