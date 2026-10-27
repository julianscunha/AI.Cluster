#!/usr/bin/env bash
# Sobe a imagem x86_64 do AI Cluster OS num QEMU UEFI (OVMF) dentro de um contêiner, na rede Docker
# `aic-net`, para validar boot, A/B e rollout sem hardware. TCG (sem KVM): lento, mas é o mesmo boot.
#
#   bash code/os/image/x86/run-qemu.sh [disco, padrão build/disk-x86.img] [nome do contêiner, padrão aic-vm]
# O console serial vai para `docker logs <nome>`. A VM usa uma cópia do disco (o original fica intacto; `docker restart` reinicia a VM mantendo o estado do disco)
# e a rede slirp: alcança o controller por IP/nome da rede aic-net.
set -euo pipefail
cd "$(dirname "$0")/.."
DISK=${1:-build/disk-x86.img}; NAME=${2:-aic-vm}
docker rm -f "$NAME" >/dev/null 2>&1 || true
docker network create aic-net >/dev/null 2>&1 || true
docker image inspect aic-x86-qemu:1 >/dev/null 2>&1 || docker build -q -f x86/Dockerfile.qemu -t aic-x86-qemu:1 x86 >/dev/null
MSYS_NO_PATHCONV=1 docker run -d --name "$NAME" --network aic-net -v "$(pwd -W 2>/dev/null || pwd)/build:/b" aic-x86-qemu:1 sh -c "
  [ -f /tmp/disk.img ] || cp /b/$(basename "$DISK") /tmp/disk.img
  [ -f /tmp/vars.fd ] || cp /usr/share/OVMF/OVMF_VARS_4M.fd /tmp/vars.fd
  exec qemu-system-x86_64 -machine q35 -cpu max -smp 2 -m 1024 -nographic \
    -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd \
    -drive if=pflash,format=raw,file=/tmp/vars.fd \
    -drive file=/tmp/disk.img,format=raw,if=virtio \
    -netdev user,id=n0,hostfwd=tcp::2222-:22 -device virtio-net-pci,netdev=n0 \
    -device i6300esb -action watchdog=reset" >/dev/null
echo "VM iniciando em $NAME; console: docker logs -f $NAME"
