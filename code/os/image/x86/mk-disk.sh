#!/usr/bin/env bash
# Monta a imagem de disco x86_64 UEFI do AI Cluster OS (GPT, A/B) a partir de build/rootfs-x86.ext4.
#
#   bash code/os/image/x86/mk-disk.sh --controller URL --token T --node-id ID [--insecure] [--ssh-key arq.pub]
#
# Layout (GPT): p1 ESP FAT32 64 MiB (BOOTX64.EFI + EFI/aic/grubenv) | p2 slot A 1 GiB | p3 slot B 1 GiB |
# p4 `aic-data` o resto (identidade do worker, sobrevive à troca de slot). A identidade do slot é o número
# da partição no disco da ESP; as PARTUUID são sorteadas a cada imagem. O rootfs vai igual nos dois
# slots (o hash assinado cobre o ext4 inteiro). Saída: build/disk-x86.img (bruto). Nada é montado no PC.
set -euo pipefail
cd "$(dirname "$0")/.."
CONTROLLER=; TOKEN=; NODE=; INSECURE=0; SSHKEY=; DISK_MIB=${DISK_MIB:-3072}
while [ $# -gt 0 ]; do
  case $1 in
    --controller) CONTROLLER=$2; shift 2;; --token) TOKEN=$2; shift 2;; --node-id) NODE=$2; shift 2;;
    --insecure) INSECURE=1; shift;; --ssh-key) SSHKEY=$2; shift 2;; *) echo "argumento desconhecido: $1" >&2; exit 2;;
  esac
done
[ -n "$CONTROLLER" ] && [ -n "$TOKEN" ] && [ -n "$NODE" ] || { echo "falta --controller, --token ou --node-id" >&2; exit 2; }
# os valores entram num arquivo que o root carrega no boot: só caracteres seguros
for v in "$CONTROLLER" "$TOKEN" "$NODE"; do case "$v" in *[!A-Za-z0-9._:/@%+=-]*) echo "valor com caractere não permitido: $v" >&2; exit 2;; esac; done
umask 077   # o .env temporário e a imagem carregam o token de join
trap 'rm -f build/worker-x86.env build/authorized_keys-x86' EXIT
[ -f build/rootfs-x86.ext4 ] || { echo "falta build/rootfs-x86.ext4 (rode x86/build-rootfs.sh)" >&2; exit 2; }
cp x86/grub.cfg build/grub-x86.cfg
{
  echo "AI_CLUSTER_CONTROLLER=$CONTROLLER"; echo "AI_CLUSTER_JOIN_TOKEN=$TOKEN"; echo "AI_CLUSTER_NODE_ID=$NODE"
  echo "AI_CLUSTER_NODE_CREDENTIAL_FILE=/data/state/credential"; echo "AI_CLUSTER_MODEL_DIR=/data/models"
  echo "AI_CLUSTER_UPDATE_CMD=/usr/sbin/aic-update-request"
  [ "$INSECURE" = 1 ] && echo "AI_CLUSTER_INSECURE=1"
  true
} > build/worker-x86.env
[ -z "$SSHKEY" ] || cp "$SSHKEY" build/authorized_keys-x86
docker image inspect aic-x86-tools:1 >/dev/null 2>&1 || docker build -q -f x86/Dockerfile.tools -t aic-x86-tools:1 x86 >/dev/null
MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd)/build:/b" aic-x86-tools:1 sh -c "
  set -e
  cd /tmp
  # BOOTX64.EFI com a configuração embutida
  grub-mkstandalone -O x86_64-efi -o BOOTX64.EFI --modules='part_gpt fat ext2 loadenv test regexp probe normal linux echo sleep reboot' 'boot/grub/grub.cfg=/b/grub-x86.cfg' 2>/dev/null
  # ESP: grubenv de 1024 bytes (slot A confirmado), pré-criado para o save_env gravar no lugar
  P1N=131072; P2N=2097152; P3N=2097152; P1S=2048; P2S=\$((P1S+P1N)); P3S=\$((P2S+P2N)); P4S=\$((P3S+P3N))
  TOTAL=\$(( $DISK_MIB * 2048 )); P4N=\$(( TOTAL - P4S - 2048 ))
  printf '# GRUB Environment Block\naic_slot=A\naic_try=\naic_tries=\n' > env.txt
  { cat env.txt; head -c \$((1024 - \$(wc -c < env.txt))) /dev/zero | tr '\0' '#'; } > grubenv
  mkfs.vfat -F 32 -n AICESP -C esp.img \$((P1N/2)) >/dev/null
  mmd -i esp.img ::EFI ::EFI/BOOT ::EFI/aic
  mcopy -i esp.img BOOTX64.EFI ::EFI/BOOT/BOOTX64.EFI
  mcopy -i esp.img grubenv ::EFI/aic/grubenv
  # dados
  mkdir -p d/state d/ssh d/models
  cp /b/worker-x86.env d/worker.env; chmod 600 d/worker.env
  [ ! -f /b/authorized_keys-x86 ] || { cp /b/authorized_keys-x86 d/ssh/authorized_keys; chmod 700 d/ssh; chmod 600 d/ssh/authorized_keys; }
  mke2fs -q -t ext4 -O ^orphan_file -L aic-data -d d data.img \$(((P4N-16)/2))K
  # disco
  truncate -s \${TOTAL}s disk.img 2>/dev/null || truncate -s \$(( TOTAL * 512 )) disk.img
  sgdisk -o -n 1:\$P1S:+\$((P1N-1)) -t 1:ef00 -c 1:esp -n 2:\$P2S:+\$((P2N-1)) -t 2:8300 -c 2:aic-slot-a -n 3:\$P3S:+\$((P3N-1)) -t 3:8300 -c 3:aic-slot-b -n 4:\$P4S:+\$((P4N-1)) -t 4:8300 -c 4:aic-data -G disk.img >/dev/null
  dd if=esp.img          of=disk.img bs=512 seek=\$P1S conv=notrunc status=none
  dd if=/b/rootfs-x86.ext4 of=disk.img bs=512 seek=\$P2S conv=notrunc status=none
  dd if=/b/rootfs-x86.ext4 of=disk.img bs=512 seek=\$P3S conv=notrunc status=none
  dd if=data.img         of=disk.img bs=512 seek=\$P4S conv=notrunc status=none
  sgdisk -p disk.img
  cp disk.img /b/disk-x86.img
"
chmod 600 build/disk-x86.img
ls -l build/disk-x86.img
