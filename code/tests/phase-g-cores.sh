#!/usr/bin/env bash
# Fatia da Fase G (G4): o scheduler local decide os cores e a unidade realmente os usa.
#
# Critério verificado: a mesma unidade, no mesmo host, roda mais rápido quando o nó
# reporta mais threads — porque o worker passa a executar a unidade em N threads em vez
# de uma. Antes disso o worker anunciava N threads e computava com uma só: o cpu_threads
# reportado era decorativo.
#
# Sequência, com a mesma unidade 512x512 em cada fase, um worker por vez para não
# disputar CPU:
#   A) nó reporta 1 thread  -> o scheduler local concede 1  -> compute lento
#   B) nó reporta 8 threads -> concede min(8, cores do host) -> compute muito menor
# O checksum tem de ser idêntico nas duas: a divisão por linhas preserva a ordem de
# acumulação, então paralelizar não muda o resultado, só o tempo.
#
# O host precisa estar ocioso: a comparação é de tempo de parede, e qualquer carga
# concorrente tira cores do nó que reporta mais threads, que é justamente o lado que
# depende de paralelismo.
set -uo pipefail

NET=aic-net
IMG=aic-rust:1
R=/t/release
CONTROLLER=http://aic-controller:50051
SIZE=${SIZE:-512}
NARROW_THREADS=${NARROW_THREADS:-1}
WIDE_THREADS=${WIDE_THREADS:-8}

cleanup() {
  docker rm -f aic-controller aic-worker-narrow aic-worker-wide >/dev/null 2>&1
  docker network rm "$NET" >/dev/null 2>&1
}

run() {
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_AUTO_APPROVE=1 --rm --network "$NET" -v aic-target:/t \
    -e AI_CLUSTER_CONTROLLER=$CONTROLLER "$IMG" sh -c "$1"
}

# Sobe controller + um worker que reporta $2 threads, roda a unidade e imprime na saída
# padrão "compute_us checksum" — uma linha só, para o chamador capturar. O que é para o
# humano vai para stderr.
measure() {
  local name=$1 threads=$2 out compute_us checksum controller_log
  cleanup
  docker network create "$NET" >/dev/null
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name aic-controller --network "$NET" \
    -v aic-target:/t "$IMG" sh -c "exec $R/ai-cluster-controller" >/dev/null
  MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_AUTO_APPROVE=1 -d --name "aic-worker-$name" --network "$NET" \
    -v aic-target:/t -e AI_CLUSTER_CONTROLLER=$CONTROLLER \
    -e AI_CLUSTER_NODE_ID="node-$name" \
    -e AI_CLUSTER_CPU_THREADS="$threads" \
    "$IMG" sh -c "exec $R/ai-cluster-worker" >/dev/null
  sleep 8

  out=$(run "$R/ai-cluster benchmark --size $SIZE --repetitions 1")

  {
    echo "--- fase $name: o nó reporta $threads threads ---"
    echo "--- quantos cores o worker recebeu do scheduler local ---"
    docker logs "aic-worker-$name" 2>&1 | grep "compute_done" \
      || echo "(o worker não registrou o grant — binário antigo?)"
    echo "$out" | grep -E "TRABALHO|^w-|concluidas="
  } >&2

  compute_us=$(echo "$out" | grep -oE "media_compute_us=[0-9]+" | cut -d= -f2)
  # The checksum is only in the controller's completion line; the CLI table has no
  # checksum column. Dump the tail when it is missing so the failure is readable.
  controller_log=$(docker logs aic-controller 2>&1)
  checksum=$(echo "$controller_log" | grep -oE "checksum=-?[0-9.]+" | tail -1 | cut -d= -f2)
  if [ -z "$checksum" ]; then
    {
      echo "--- (diagnóstico) o log do controller não trouxe checksum; últimas linhas: ---"
      echo "$controller_log" | tail -6
    } >&2
  fi
  echo "$compute_us $checksum"
}

cleanup
echo "=== host: $(MSYS_NO_PATHCONV=1 docker run -e AI_CLUSTER_JOIN_TOKEN=${AI_CLUSTER_JOIN_TOKEN:-e2e-token} -e AI_CLUSTER_AUTO_APPROVE=1 --rm "$IMG" nproc) cores visíveis no container ===" >&2

read -r NARROW_US NARROW_CK <<<"$(measure narrow "$NARROW_THREADS")"
read -r WIDE_US WIDE_CK <<<"$(measure wide "$WIDE_THREADS")"

echo "=== resultado ==="
echo "narrow (reporta $NARROW_THREADS): compute_us=$NARROW_US checksum=$NARROW_CK"
echo "wide   (reporta $WIDE_THREADS): compute_us=$WIDE_US checksum=$WIDE_CK"

RC=0
if [ -z "$NARROW_US" ] || [ -z "$WIDE_US" ] || [ -z "$NARROW_CK" ] || [ -z "$WIDE_CK" ]; then
  echo "FALHA: não consegui ler compute_us/checksum da saída do CLI"
  RC=1
elif [ "$NARROW_CK" != "$WIDE_CK" ]; then
  echo "FALHA: o checksum mudou com o número de threads; a paralelização não preservou o resultado"
  RC=1
else
  echo "OK: checksum idêntico com $NARROW_THREADS e com $WIDE_THREADS threads reportadas"
  if [ "$WIDE_US" -ge "$NARROW_US" ]; then
    echo "FALHA: o nó que reporta mais threads não computou mais rápido"
    RC=1
  else
    echo "OK: compute caiu de ${NARROW_US}us para ${WIDE_US}us ao conceder mais cores"
  fi
fi

cleanup
echo "=== fim (rc=$RC) ==="
exit "$RC"
