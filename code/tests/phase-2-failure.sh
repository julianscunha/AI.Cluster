#!/usr/bin/env bash
# Fatia da Fase 2: tolerância a falhas — um worker morre e o DAG ainda termina.
#
# O critério de aceitação verificado: perder um worker não corrompe o DAG.
# O worker é morto sem saída graciosa, então o controller só percebe quando
# sua graça de heartbeat expira. O que o nó morto segurava deve terminar ou
# reenfileirado no nó sobrevivente ou falhado de forma limpa — nunca deixado pendente.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051
# O tamanho fica abaixo do teto de mensagem do gRPC; mantenha-o lá.
SIZE=${SIZE:-256}
KILL_AFTER=${KILL_AFTER:-10}
# Segure cada unidade reivindicada antes de computá-la, para que o kill caia com uma unidade em
# voo. Sem isso a janela em voo tem poucos microssegundos e o teste só
# exercita o caminho ocioso. Mais longo que o intervalo de heartbeat de 5s de propósito:
# uma vez que um worker reivindicou, ele segura a unidade quando o kill chega.
WORK_DELAY_MS=${WORK_DELAY_MS:-12000}

run() {
  MSYS_NO_PATHCONV=1 docker run --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

echo "=== limpeza ==="
docker rm -f aic-controller aic-worker-b aic-worker-c >/dev/null 2>&1
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null

echo "=== iniciar controller + dois workers ==="
# Graça deliberadamente menor que a retenção de trabalho abaixo. Antes de o heartbeat ser
# desacoplado do laço de trabalho, isso ceifava o worker saudável no meio da unidade; a execução
# agora só deve ceifar o nó que foi morto.
MSYS_NO_PATHCONV=1 docker run -d --name aic-controller --network "$NET" \
  -v aic-target:/t -e AI_CLUSTER_HEARTBEAT_GRACE_MS=9000 \
  "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
for n in b c; do
  MSYS_NO_PATHCONV=1 docker run -d --name "aic-worker-$n" --network "$NET" \
    -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID="node-$n" \
    -e AI_CLUSTER_WORK_DELAY_MS="$WORK_DELAY_MS" \
    "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
done
sleep 8

echo "--- nós antes da falha ---"
run "$R/ai-cluster nodes"

echo "=== enfileirar duas unidades para que ambos os workers segurem uma, então matar um worker em voo ==="
# Duas unidades prontas e dois workers: cada worker reivindica uma, então o nó que morre
# está de fato segurando trabalho. O diamante não faria isso — ele tem uma única cabeça pronta,
# então só um worker pode estar ocupado por vez.
run "$R/ai-cluster benchmark --size "$SIZE" --repetitions 2" &
CLI=$!
sleep "$KILL_AFTER"
echo "--- matando node-b: SIGKILL, sem leave, sem requeue na saída ---"
docker kill aic-worker-b >/dev/null 2>&1
wait "$CLI"

echo "--- nós após a falha ---"
run "$R/ai-cluster nodes"

echo "--- log do controller ---"
docker logs aic-controller 2>&1 | tail -20
