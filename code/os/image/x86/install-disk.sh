#!/bin/sh
# Grava a imagem x86_64 do AI Cluster OS (build/disk-x86.img, de mk-disk.sh) num disco e usa o disco
# inteiro: move a GPT de backup para o fim, sorteia os GUIDs e estende a partição de dados.
#
#   x86/install-disk.sh <imagem> <dispositivo> --yes
#
# Rode como root num Linux qualquer (um pendrive de recuperação serve), com o disco de destino
# desmontado. APAGA O DISCO INTEIRO. Precisa de: dd, sgdisk (gdisk), e2fsck e resize2fs (e2fsprogs).
set -eu
IMG=${1:-}; DEV=${2:-}; YES=${3:-}
[ -f "$IMG" ] && [ -b "$DEV" ] || { echo "uso: $0 <imagem> <dispositivo-de-bloco> --yes" >&2; exit 2; }
[ "$YES" = --yes ] || { echo "isto apaga $DEV inteiro; confirme com --yes" >&2; exit 2; }
for t in dd sgdisk e2fsck resize2fs; do command -v "$t" >/dev/null || { echo "falta $t" >&2; exit 2; }; done
# nunca no disco que está em uso
if grep -q "^$DEV" /proc/mounts || grep -q "^${DEV}p\?[0-9]" /proc/mounts; then echo "$DEV tem partição montada; abortando" >&2; exit 2; fi
SIZE=$(blockdev --getsize64 "$DEV"); NEED=$(stat -c %s "$IMG")
[ "$SIZE" -ge "$NEED" ] || { echo "disco menor que a imagem" >&2; exit 2; }

dd if="$IMG" of="$DEV" bs=4M conv=fsync status=progress
sgdisk -e "$DEV" >/dev/null                     # GPT de backup no fim do disco
sgdisk -G "$DEV" >/dev/null                     # GUIDs novos: dois discos com a mesma imagem não colidem
# partição de dados (4): mesmo início, até o fim do disco
START=$(sgdisk -i 4 "$DEV" | sed -n 's/^First sector: \([0-9]*\).*/\1/p')
sgdisk -d 4 -n "4:$START:0" -t 4:8300 -c 4:aic-data "$DEV" >/dev/null
partprobe "$DEV" 2>/dev/null || partx -u "$DEV" 2>/dev/null || blockdev --rereadpt "$DEV" || true
sleep 1
P4=$(ls "${DEV}"4 "${DEV}p4" 2>/dev/null | head -n 1)
[ -b "$P4" ] || { echo "o kernel não enxergou a partição 4 de $DEV; reinicie e rode: e2fsck -f ${DEV}4 && resize2fs ${DEV}4" >&2; exit 1; }
e2fsck -fy "$P4" >/dev/null || true
resize2fs "$P4" >/dev/null
sync
echo "instalado em $DEV; desligue, remova o pendrive de recuperação e dê boot pelo disco (UEFI, Secure Boot desligado)"
