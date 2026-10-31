#!/usr/bin/env bash
# Monta a imagem de disco do AI Cluster OS (F15 J4/J5) com o layout A/B para um cartão de 3.933.208.576
# bytes (o do Orange Pi PC de bancada); ajuste CARD_BYTES para outro tamanho.
#
#   bash code/os/image/mk-disk-image.sh <uboot-header.bin> <zImage> <dtb>
#
#   uboot-header.bin  o primeiro 1 MiB de um cartão que já dá boot na placa (carrega o u-boot, que
#                     fica nos setores 16 em diante: não é redistribuível aqui, vem do próprio cartão)
#   zImage, dtb       o kernel e a árvore de dispositivos da placa (do próprio sistema dela)
#
# Layout (setores de 512 B): [0..2047] MBR + u-boot | p1 FAT32 `AICBOOT` 64 MiB (kernel, dtb, boot.scr) | p2 `aicroot` slot A 512 MiB | p3 `aicroot` slot B 512 MiB | p4 `aicdata` o resto
# (credencial do worker e worker.env: sobrevivem à troca de slot).
# Precisa de build/rootfs.ext4 (build-rootfs.sh). Saída: build/disk.img.gz. Nada é montado no PC.
set -euo pipefail
cd "$(dirname "$0")"
HEADER=$1; ZIMAGE=$2; DTB=$3
CARD_BYTES=${CARD_BYTES:-3933208576}
[ -f build/rootfs.ext4 ] || { echo "falta build/rootfs.ext4 (rode build-rootfs.sh)" >&2; exit 2; }
for f in "$HEADER" "$ZIMAGE" "$DTB"; do [ -f "$f" ] || { echo "arquivo não encontrado: $f" >&2; exit 2; }; done
cp "$HEADER" build/uboot-header.bin; cp "$ZIMAGE" build/zImage; cp "$DTB" build/dtb; cp boot.cmd build/boot.cmd

docker volume rm aic-image >/dev/null 2>&1 || true
MSYS_NO_PATHCONV=1 docker run --rm -v aic-image:/o -v "$(pwd -W 2>/dev/null || pwd)/build:/b:ro" debian:12-slim sh -c "
  set -e
  apt-get update -qq >/dev/null && apt-get install -y -qq e2fsprogs dosfstools mtools u-boot-tools fdisk >/dev/null
  cd /o
  TOTAL=\$(( $CARD_BYTES / 512 ))
  P1S=2048;  P1N=131072
  P2S=\$((P1S + P1N)); P2N=1048576
  P3S=\$((P2S + P2N)); P3N=1048576
  P4S=\$((P3S + P3N)); P4N=\$((TOTAL - P4S))

  # boot: FAT32 com kernel, dtb, boot.scr e o estado do A/B (slot A confirmado, sem teste)
  mkimage -C none -A arm -T script -d /b/boot.cmd boot.scr >/dev/null
  mkfs.vfat -F 32 -n AICBOOT -C boot.img \$(( P1N / 2 )) >/dev/null
  mcopy -i boot.img /b/zImage /b/dtb boot.scr ::

  # dados: ext4 com os diretórios que o sistema espera
  mkdir -p d/state d/ssh
  mke2fs -q -t ext4 -L aicdata -d d data.img \$(( P4N / 2 ))K

  # disco: cabeçalho (MBR + u-boot), tabela de partições e as quatro partições
  truncate -s $CARD_BYTES disk.img
  dd if=/b/uboot-header.bin of=disk.img bs=1M count=1 conv=notrunc status=none
  printf 'label: dos\nunit: sectors\n\n%s,%s,c,*\n%s,%s,83\n%s,%s,83\n%s,,83\n' \
    \$P1S \$P1N \$P2S \$P2N \$P3S \$P3N \$P4S | sfdisk --no-reread --no-tell-kernel disk.img >/dev/null
  # estado do A/B (slot A confirmado) no setor 2016, na folga entre o u-boot e a primeira partição
  printf 'aic_slot=A\naic_try=\naic_tries=\naic_boots=\n' | dd of=disk.img bs=512 seek=2016 count=1 conv=notrunc,sync status=none
  dd if=boot.img        of=disk.img bs=512 seek=\$P1S conv=notrunc status=none
  dd if=/b/rootfs.ext4  of=disk.img bs=512 seek=\$P2S conv=notrunc status=none
  dd if=/b/rootfs.ext4  of=disk.img bs=512 seek=\$P3S conv=notrunc status=none
  dd if=data.img        of=disk.img bs=512 seek=\$P4S conv=notrunc status=none
  sfdisk -l disk.img
  gzip -1 -c disk.img > disk.img.gz
  ls -l disk.img.gz
"
MSYS_NO_PATHCONV=1 docker run --rm -v aic-image:/o -v "$(pwd -W 2>/dev/null || pwd)/build:/b" debian:12-slim cp /o/disk.img.gz /b/disk.img.gz
ls -l build/disk.img.gz
