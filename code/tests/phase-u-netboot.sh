#!/usr/bin/env bash
# Fatia da F15 (J4/J5): reimage por boot de rede, com retorno automático ao slot local.
#
# Teste de bancada: precisa de um worker Armbian/u-boot real na LAN que já tenha o boot pela rede
# instalado (os/netboot/install-netboot.sh install ... com prazo curto, p.ex. 150 s), a imagem em
# code/os/netboot/srv/netboot/{zImage,initramfs.cpio.gz,dtb} (os/netboot/build-initramfs.sh), o
# controller no ar e um worker local desse worker registrado com LOCAL_NODE. Nada aqui apaga o cartão.
#
# Critérios verificados:
#   A) com o TFTP no ar, o worker reinicia, baixa a imagem e o worker da rede (NET_NODE) fica READY sem
#      que o cartão seja tocado (J4: o sistema novo entra no cluster sozinho);
#   B) passado o prazo, o worker reinicia e, como o servidor só entrega a imagem uma vez, sobe do
#      cartão local e o LOCAL_NODE volta a READY (J5: recuperação para o slot A, sem intervenção);
#   C) o servidor registrou a recusa da segunda entrega (o limite de boots segurou).
#
# Variáveis: PI_SSH (comando que roda uma linha no worker; padrão "ssh root@$PI_HOST"), PI_HOST,
# PI_IP_ALLOW (IP que pode baixar, padrão = PI_HOST), NET_NODE, LOCAL_NODE, CONTROLLER,
# OPERATOR_TOKEN, TTL_WAIT (segundos máximos de espera pelo retorno; padrão 420).
set -uo pipefail

PI_HOST=${PI_HOST:-192.168.1.24}
PI_SSH=${PI_SSH:-ssh root@$PI_HOST}
ALLOW=${PI_IP_ALLOW:-$PI_HOST}
NET_NODE=${NET_NODE:-orangepi-net}
LOCAL_NODE=${LOCAL_NODE:-orangepi-boot}
CONTROLLER=${CONTROLLER:-http://host.docker.internal:50051}
OPERATOR=${OPERATOR_TOKEN:-lan-op}
TTL_WAIT=${TTL_WAIT:-420}
HERE=$(cd "$(dirname "$0")/.." && pwd)
LOG=$(mktemp)
PY=$(command -v python3 || command -v python)

cli() {
  MSYS_NO_PATHCONV=1 docker run --rm -v aic-target:/t -e AI_CLUSTER_INSECURE=1 \
    -e AI_CLUSTER_OPERATOR_TOKEN="$OPERATOR" -e AI_CLUSTER_CONTROLLER="$CONTROLLER" \
    aic-rust:1 /t/release/aic cluster "$@" 2>/dev/null
}
fail() { echo "FALHA: $1"; kill "${TFTP:-0}" 2>/dev/null; exit 1; }
ready() { cli nodes | grep -E "^$1 " | grep -q READY; }
wait_ready() { # wait_ready <worker> <segundos>
  for _ in $(seq 1 $(($2 / 5))); do ready "$1" && return 0; sleep 5; done
  return 1
}

[ -f "$HERE/os/netboot/srv/netboot/initramfs.cpio.gz" ] || fail "faltam os artefatos em os/netboot/srv/netboot"
"$PY" "$HERE/os/netboot/tftpd.py" --allow "$ALLOW" --boots 1 >"$LOG" 2>&1 &
TFTP=$!
sleep 2
echo "=== A) o worker reinicia, baixa a imagem e o worker da rede fica READY ==="
$PI_SSH '(sleep 2; reboot) >/dev/null 2>&1 &' >/dev/null 2>&1
wait_ready "$NET_NODE" 360 || { cat "$LOG"; fail "$NET_NODE não ficou READY pelo boot de rede"; }
grep -q "netboot/dtb: concluido" "$LOG" || fail "o TFTP deveria ter entregado a imagem inteira"
echo "OK: $NET_NODE READY depois de baixar zImage, initramfs e dtb"

echo "=== B) vencido o prazo, o worker volta sozinho ao cartão local ==="
wait_ready "$LOCAL_NODE" "$TTL_WAIT" || { cat "$LOG"; fail "$LOCAL_NODE não voltou a READY depois do prazo"; }
echo "OK: $LOCAL_NODE voltou a READY sem intervenção"

echo "=== C) o limite de boots segurou a segunda entrega ==="
grep -q "limite de 1 boot(s) atingido" "$LOG" || { cat "$LOG"; fail "o servidor deveria ter recusado a segunda entrega"; }
echo "OK: a segunda entrega foi recusada"

kill "$TFTP" 2>/dev/null
rm -f "$LOG"
echo "=== fim ==="
