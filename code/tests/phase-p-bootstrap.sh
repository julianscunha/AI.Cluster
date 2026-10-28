#!/usr/bin/env bash
# Fatia da Fase P (F15, J3): bootstrap de um Linux existente até virar worker.
#
# Critério verificado: um Debian limpo, sem Rust e sem nada do cluster, recebe só o binário do
# worker e o `os/bootstrap.sh` e vira um nó READY no controller, sem outra intervenção.
# (O Debian roda num contêiner, sem systemd: o script usa o caminho "segundo plano".)
#
# Sequência:
#   A) argumentos errados (inclusive caracteres de shell)  são recusados antes de mexer na máquina;
#   B) o bootstrap instala, grava o ambiente em modo 0600 e o worker aparece READY;
#   C) rodar de novo é inofensivo (idempotente) e o nó segue um só.
set -uo pipefail

NET=aic-net-p
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller-p:50051
OS="$PWD/os"

teardown() { docker rm -f aic-controller-p aic-clean-p >/dev/null 2>&1; }
fail() { echo "FALHA: $1"; teardown; exit 1; }
cli() {
  MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" -v aic-target:/t -e AI_CLUSTER_INSECURE=1 \
    -e AI_CLUSTER_OPERATOR_TOKEN=p-operator -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$R/ai-cluster $*"
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null
MSYS_NO_PATHCONV=1 docker run -d --name aic-controller-p --network "$NET" -v aic-target:/t \
  -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=p-join -e AI_CLUSTER_OPERATOR_TOKEN=p-operator \
  -e AI_CLUSTER_AUTO_APPROVE=1 "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null

# A máquina "limpa": só o Debian, com o binário e o script montados somente para leitura.
MSYS_NO_PATHCONV=1 docker run -d --name aic-clean-p --network "$NET" \
  -v aic-target:/t:ro -v "$OS":/os:ro debian:bookworm-slim sleep 600 >/dev/null || fail "não consegui subir o Debian"
boot() { MSYS_NO_PATHCONV=1 docker exec aic-clean-p /os/bootstrap.sh "$@"; }
# O script recusa argumento ruim com o código 2; qualquer outro código (inclusive falha do docker) não conta.
refused() { boot "$@" >/dev/null 2>&1; [ $? = 2 ]; }

echo "=== A) argumentos recusados antes de tocar na máquina ==="
refused --controller http://aic-controller-p:50051 --join-token p-join --binary /t/release/ai-cluster-worker   || fail "http sem --insecure deveria ser recusado (código 2)"
refused --controller https://x --binary /t/release/ai-cluster-worker || fail "sem token deveria ser recusado (código 2)"
refused --controller https://x --join-token "a b" --binary /t/release/ai-cluster-worker   || fail "token com espaço deveria ser recusado (código 2)"
refused --controller https://x --join-token t --binary /nao/existe || fail "binário ausente deveria ser recusado (código 2)"
# Caracteres de shell ou quebra de linha em qualquer valor: recusados antes de escrever nada.
for bad in 'a$(id)' 'a`id`' 'a' 'a#b' 'a;b' "a
b"; do
  refused --controller https://x --join-token "$bad" --binary /t/release/ai-cluster-worker     || fail "token com caractere perigoso deveria ser recusado"
  refused --controller https://x --join-token t --node-id "$bad" --binary /t/release/ai-cluster-worker     || fail "node-id com caractere perigoso deveria ser recusado"
  refused --controller https://x --join-token t --model-dir "$bad" --binary /t/release/ai-cluster-worker     || fail "model-dir com caractere perigoso deveria ser recusado"
  refused --controller https://x --join-token t --prefix "$bad" --binary /t/release/ai-cluster-worker     || fail "prefix com caractere perigoso deveria ser recusado"
done
refused --controller https://x --join-token t --prefix relativo --binary /t/release/ai-cluster-worker   || fail "prefix relativo deveria ser recusado"
# RPC do llama.cpp: tudo ou nada, binário executável, os três arquivos de TLS, nada de texto puro.
refused --controller https://x --join-token t --binary /t/release/ai-cluster-worker --rpc-server-bin /t/llama/ggml-rpc-server   || fail "RPC sem pasta de TLS deveria ser recusado"
refused --controller https://x --join-token t --binary /t/release/ai-cluster-worker --rpc-tls-dir /tmp   || fail "RPC sem binário deveria ser recusado"
refused --controller https://x --join-token t --binary /t/release/ai-cluster-worker --rpc-server-bin /nao/existe --rpc-tls-dir /tmp   || fail "binário de RPC ausente deveria ser recusado"
refused --controller https://x --join-token t --binary /t/release/ai-cluster-worker --rpc-server-bin /t/llama/ggml-rpc-server --rpc-tls-dir /tmp   || fail "pasta de TLS sem os arquivos deveria ser recusada"
MSYS_NO_PATHCONV=1 docker exec aic-clean-p test ! -e /opt/ai-cluster || fail "nada deveria ter sido instalado"
echo "OK"

echo "=== B) instala e vira READY ==="
# O token por arquivo: nada de segredo na linha de comando.
MSYS_NO_PATHCONV=1 docker exec aic-clean-p sh -c 'echo p-join > /tmp/join-token'
boot --controller $CONTROLLER --join-token-file /tmp/join-token --insecure --node-id node-clean \
  --binary /t/release/ai-cluster-worker || fail "o bootstrap falhou"
MODE=$(MSYS_NO_PATHCONV=1 docker exec aic-clean-p stat -c '%a %U:%G' /opt/ai-cluster/worker.env)
[ "$MODE" = "640 root:ai-cluster" ] || fail "worker.env deveria ser 640 root:ai-cluster, veio '$MODE'"
BIN=$(MSYS_NO_PATHCONV=1 docker exec aic-clean-p stat -c '%a %U' /opt/ai-cluster/bin/ai-cluster-worker)
[ "$BIN" = "755 root" ] || fail "o binário deveria ser 755 do root, veio '$BIN'"
STATE=""
for _ in $(seq 1 20); do
  STATE=$(cli nodes 2>/dev/null | awk '$1 == "node-clean" { print $2 }')
  [ "$STATE" = READY ] && break
  sleep 1
done
[ "$STATE" = READY ] || fail "node-clean deveria estar READY, está '$STATE'"
PID=$(MSYS_NO_PATHCONV=1 docker exec aic-clean-p cat /opt/ai-cluster/state/worker.pid)
[ "$(MSYS_NO_PATHCONV=1 docker exec aic-clean-p stat -c %U /proc/$PID)" = ai-cluster ] || fail "o worker deveria rodar como ai-cluster"
echo "OK: node-clean READY, worker.env 640 root:ai-cluster, binário do root, processo do ai-cluster"

echo "=== C) rodar de novo é inofensivo ==="
boot --controller $CONTROLLER --join-token p-join --insecure --node-id node-clean \
  --binary /t/release/ai-cluster-worker >/dev/null || fail "a segunda execução falhou"
sleep 6
COUNT=$(cli nodes 2>/dev/null | awk '$1 == "node-clean"' | wc -l)
[ "$COUNT" = 1 ] || fail "deveria haver um nó node-clean, há $COUNT"
# O nome do processo no /proc é cortado em 15 caracteres ("ai-cluster-work"). O contêiner não tem
# init que recolha filhos, então o worker antigo, já encerrado, fica como zumbi: não conta.
RUNNING=$(MSYS_NO_PATHCONV=1 docker exec aic-clean-p sh -c 'for d in /proc/[0-9]*; do grep -q "^ai-cluster-work" $d/comm 2>/dev/null && ! grep -q "^State:.*Z" $d/status && echo x; done | wc -l')
[ "$RUNNING" = 1 ] || fail "deveria haver um processo do worker, há $RUNNING"
echo "OK: um nó, um processo"

echo "=== D) trocar o id da máquina exige --force ==="
refused --controller $CONTROLLER --join-token p-join --insecure --node-id outro-nome   --binary /t/release/ai-cluster-worker || fail "trocar o node-id sem --force deveria ser recusado (código 2)"
echo "OK"

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== fim ==="
