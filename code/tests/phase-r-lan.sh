#!/usr/bin/env bash
# Fatia da validação em hardware real: um cluster de máquinas físicas na LAN.
#
# Diferente dos outros E2E, este NÃO sobe nada: confere um cluster que já está de pé, com ao
# menos um worker numa máquina de outra arquitetura (ex.: x86 + ARM). Para montá-lo, veja
# docs/guia/testes.md.
#
# Critérios verificados:
#   A) há pelo menos dois workers READY de arquiteturas diferentes;
#   B) um lote de unidades termina inteiro e roda em mais de um worker;
#   C) a mesma unidade dá o mesmo checksum em arquiteturas diferentes (paridade numérica).
#
# Variáveis: LAN_CONTROLLER (padrão http://host.docker.internal:50051), LAN_OPERATOR_TOKEN
# (padrão lan-op), LAN_CONTROLLER_CONTAINER (padrão aic-controller-lan, de onde vêm os checksums).
set -uo pipefail

CONTROLLER=${LAN_CONTROLLER:-http://host.docker.internal:50051}
OPERATOR=${LAN_OPERATOR_TOKEN:-lan-op}
CONTAINER=${LAN_CONTROLLER_CONTAINER:-aic-controller-lan}
UNITS=6

cli() {
  MSYS_NO_PATHCONV=1 docker run --rm -v aic-target:/t -e AI_CLUSTER_INSECURE=1 \
    -e AI_CLUSTER_OPERATOR_TOKEN="$OPERATOR" -e AI_CLUSTER_CONTROLLER="$CONTROLLER" \
    aic-rust:1 /t/release/aic "$@"
}

fail() { echo "FALHA: $1"; exit 1; }

echo "=== A) workers READY de arquiteturas diferentes ==="
NODES=$(cli cluster nodes)
echo "$NODES"
ARCHS=$(echo "$NODES" | awk '$2 == "READY" { print $3 }' | sort -u | wc -l)
[ "$ARCHS" -ge 2 ] || fail "esperadas >= 2 arquiteturas entre os workers READY, vieram $ARCHS"
echo "OK: $ARCHS arquiteturas entre os workers READY"

echo "=== B) o lote termina inteiro e roda em mais de um worker ==="
BEFORE=$(docker logs "$CONTAINER" 2>&1 | grep -c "finalizado")
# Três lotes de UNITS unidades com a mesma semente: cada unidade se repete a cada UNITS ids.
for _ in 1 2 3; do cli cluster benchmark --size 96 --repetitions $UNITS >"$(mktemp)" 2>&1 || fail "o benchmark falhou"; done
RUN=$(docker logs "$CONTAINER" 2>&1 | grep "finalizado" | tail -n +$((BEFORE + 1)))
[ "$(echo "$RUN" | wc -l)" -eq $((3 * UNITS)) ] || fail "esperadas $((3 * UNITS)) unidades concluídas"
NODES_USED=$(echo "$RUN" | sed -E 's/.*finalizado em ([^ ]+) .*/\1/' | sort -u | wc -l)
[ "$NODES_USED" -ge 2 ] || fail "as unidades rodaram em $NODES_USED worker(s); esperado >= 2"
echo "OK: $((3 * UNITS)) unidades concluídas em $NODES_USED workers"

echo "=== C) o mesmo checksum em arquiteturas diferentes ==="
# unidade = id mod UNITS; para cada uma, os checksums vistos em cada worker. Só conta quem rodou em 2+ workers.
declare -A ARCH_OF
while read -r name arch; do ARCH_OF[$name]=$arch; done < <(echo "$NODES" | awk '$2 == "READY" { print $1, $3 }')
CHECKED=0
while read -r unit; do
  SUMS=$(echo "$RUN" | sed -E 's/.*trabalho w-([0-9]+) finalizado em ([^ ]+) .*checksum=(-?[0-9.]+)/\1 \2 \3/' \
    | awk -v u="$unit" -v n=$UNITS '$1 % n == u { print $2, $3 }')
  ARCHS_SEEN=$(echo "$SUMS" | while read -r node _; do echo "${ARCH_OF[$node]:-?}"; done | sort -u | wc -l)
  [ "$ARCHS_SEEN" -ge 2 ] || continue
  [ "$(echo "$SUMS" | awk '{ print $2 }' | sort -u | wc -l)" -eq 1 ] \
    || fail "a unidade $unit deu checksums diferentes entre arquiteturas: $(echo "$SUMS" | tr '\n' ' ')"
  CHECKED=$((CHECKED + 1))
done < <(seq 0 $((UNITS - 1)))
[ "$CHECKED" -ge 1 ] || fail "nenhuma unidade rodou em duas arquiteturas; rode de novo"
echo "OK: $CHECKED unidade(s) rodaram em arquiteturas diferentes com checksum idêntico"
echo "=== fim ==="
