#!/usr/bin/env bash
# Fatia da Fase I (I4): promoção de réplica — o volume do controller morre, a cópia num worker não.
#
# Critério verificado: a fila sobrevive à perda dos DOIS arquivos locais do controller, porque
# um worker guarda uma cópia do estado e a oferece de volta ao (re-)registrar. Nenhum roteiro
# cobria isto: `phase-i-state.sh` cobre a cópia local ao lado do primário, e as duas moram no
# mesmo volume — quando o volume morre, morrem juntas. É essa a falha que a cópia num worker cobre.
#
# Sequência:
#   A) controller com estado + worker com `AI_CLUSTER_STATE_REPLICA`: o worker guarda a cópia.
#   B) uma requisição é enfileirada: a gravação data o estado e a batida leva a cópia ao worker.
#   C) o controller morre; os dois arquivos locais são apagados — o volume morreu com eles.
#   D) o controller volta vazio; o worker re-registra, oferece a cópia, e a fila volta.
#   E) um worker novo drena o trabalho recuperado.
#
# Nota de honestidade: o volume é compartilhado, então o arquivo do worker está no mesmo disco do
# que os arquivos do controller — o que este roteiro prova é que a *fonte* da fila recuperada
# é a cópia do worker e não um arquivo do controller. Em produção a cópia fica no disco do worker, e é
# isso que a faz sobreviver à morte do volume do controller. Ver "Teto conhecido" em
# `docs/system/02-subsistemas/dados/state-manager.md`.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051
STATE_DIR=/t/state-i4r
STATE=$STATE_DIR/state.bin
# O arquivo do worker. Distinto do par do controller de propósito: é o que prova de onde a fila
# recuperada veio.
NODE_REPLICA=$STATE_DIR/node-a.bin
GRACE_MS=${GRACE_MS:-6000}
# Segura a unidade reivindicada: sem isso o worker drena a fila antes de o controller morrer.
HOLD_MS=${HOLD_MS:-45000}

run() {
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

# Runs a command against the shared volume without joining the cluster network.
in_vol() {
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 --rm -v aic-target:/t "$IMG" sh -c "$1"
}

worker() {
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name "$1" --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER -e AI_CLUSTER_NODE_ID="$2" \
    -e AI_CLUSTER_WORK_DELAY_MS="$3" -e AI_CLUSTER_STATE_REPLICA="$4" \
    "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
}

start_controller() {
  # Um kill deixa o container parado segurando o nome; o restart precisa do nome livre.
  docker rm -f aic-controller >/dev/null 2>&1
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_INSECURE=1 -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_OPERATOR_TOKEN=${AI_CLUSTER_OPERATOR_TOKEN:-e2e-operator} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-controller --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_STATE=$STATE -e AI_CLUSTER_HEARTBEAT_GRACE_MS=$GRACE_MS \
    "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
}

teardown() {
  docker rm -f aic-controller aic-worker-a aic-worker-b >/dev/null 2>&1
}

teardown
docker network rm "$NET" >/dev/null 2>&1
docker network create "$NET" >/dev/null
# O controller escreve dentro do diretório, não o cria.
in_vol "rm -rf $STATE_DIR && mkdir -p $STATE_DIR"

echo "=== A) o worker guarda a cópia do estado ==="
start_controller
worker aic-worker-a node-a "$HOLD_MS" "$NODE_REPLICA"
sleep 8

echo "=== B) enfileira: a gravação leva a cópia até o worker ==="
# Em segundo plano: com o worker segurando a unidade, a fila não drena e o cliente ficaria
# esperando até o próprio deadline.
run "$R/aic cluster benchmark --size 64 --repetitions 3" >/dev/null 2>&1 &
CLI=$!
sleep 12

if in_vol "test -s $NODE_REPLICA"; then
  echo "OK: o worker guardou a cópia em $NODE_REPLICA ($(in_vol "stat -c %s $NODE_REPLICA") bytes)"
else
  echo "FALHA: esperado o worker guardar uma cópia em $NODE_REPLICA"
  docker logs aic-worker-a 2>&1 | tail -20
  teardown
  exit 1
fi

echo "=== C) o controller morre; os dois arquivos locais vão com ele ==="
docker kill aic-controller aic-worker-a >/dev/null 2>&1
wait "$CLI" 2>/dev/null || true
in_vol "rm -f $STATE $STATE.replica"
if in_vol "test -e $STATE"; then
  echo "FALHA: o arquivo do controller deveria ter sumido"
  teardown
  exit 1
fi
echo "OK: o volume do controller morreu (primário e cópia local apagados)"

echo "=== D) o controller volta vazio e o worker oferece a cópia ==="
start_controller
# O worker está morto; quem oferece a cópia é um worker novo que lê o arquivo que ficou.
worker aic-worker-b node-b 0 "$NODE_REPLICA"
sleep 12

RESTORED=$(docker logs aic-controller 2>&1 | grep "estado restaurado da réplica de um worker")
echo "$RESTORED"
LOADED=$(docker logs aic-controller 2>&1 | grep "estado carregado de" || true)
if [ -n "$LOADED" ]; then
  echo "FALHA: o controller carregou um arquivo local ($LOADED) — a promoção não foi provada"
  teardown
  exit 1
fi
if echo "$RESTORED" | grep -q "estado restaurado da réplica de um worker"; then
  echo "OK: o controller recuperou a fila da cópia de um worker"
else
  echo "FALHA: esperado 'estado restaurado da réplica de um worker' no log"
  docker logs aic-controller 2>&1 | tail -20
  docker logs aic-worker-b 2>&1 | tail -20
  teardown
  exit 1
fi
# A cópia voltou para os arquivos do controller: o próximo restart é um restart normal.
if in_vol "test -s $STATE"; then
  echo "OK: a cópia adotada foi gravada de volta em $STATE"
else
  echo "FALHA: esperado o estado adotado gravado em $STATE"
  teardown
  exit 1
fi

echo "=== E) o cluster drena o trabalho recuperado ==="
sleep 12
METRICS=$(run "$R/aic cluster metrics")
echo "$METRICS"
PENDING=$(echo "$METRICS" | sed -n 's/.*units_pending=\([0-9]*\).*/\1/p' | head -1)
FAILED=$(echo "$METRICS" | sed -n 's/.*units_failed=\([0-9]*\).*/\1/p' | head -1)
COMPLETED=$(echo "$METRICS" | sed -n 's/.*units_completed=\([0-9]*\).*/\1/p' | head -1)
if [ "$PENDING" = "0" ] && [ "$FAILED" = "0" ] && [ "$COMPLETED" = "3" ]; then
  echo "OK: as 3 unidades voltaram da cópia e terminaram"
else
  echo "FALHA: esperado concluidas=3 pendentes=0 falharam=0, veio concluidas=$COMPLETED pendentes=$PENDING falharam=$FAILED"
  docker logs aic-controller 2>&1 | tail -20
  teardown
  exit 1
fi

teardown
docker network rm "$NET" >/dev/null 2>&1
echo "=== I4 réplica OK ==="
