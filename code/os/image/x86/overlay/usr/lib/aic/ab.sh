#!/bin/sh
# A/B no x86_64 UEFI (GRUB): os slots são as partições 2 (A) e 3 (B) do MESMO disco da raiz em execução
# (nunca de outro disco com o mesmo layout) e o estado fica no grubenv da ESP (partição 1).
#   slot_dev A|B           caminho da partição do slot
#   state_get <variável>   valor atual (aic_slot, aic_try, aic_tries)
#   state_set <slot> <try> grava o slot confirmado e o slot em teste (vazio = nenhum)
ENV=/mnt/boot/EFI/aic/grubenv

# nome do disco (vda, sda, nvme0n1…) que contém a raiz montada
disk() { d=$(readlink -f "/sys/dev/block/$(mountpoint -d /)/.."); echo "${d##*/}"; }
# /dev/<partição de número $1 do disco da raiz>
part() {
	for p in /sys/block/"$(disk)"/*/partition; do
		[ "$(cat "$p")" = "$1" ] && { d=${p%/partition}; echo "/dev/${d##*/}"; return 0; }
	done
	return 1
}
slot_dev() { case "$1" in A) part 2;; B) part 3;; *) return 1;; esac; }

# a ESP é montada só no instante da leitura/escrita; o lock evita updater e commit ao mesmo tempo
with_esp() { # $1 = ro|rw, resto = comando
	mode=$1; shift
	esp=$(part 1) || return 1
	( flock 9
	  mount -t vfat -o "$mode" "$esp" /mnt/boot || exit 1
	  "$@"; rc=$?
	  umount /mnt/boot; exit $rc ) 9> /run/aic-ab.lock
}
state_get() { with_esp ro sed -n "s/^$1=//p" "$ENV"; }
# bloco de 1024 bytes gravado no lugar (como o save_env do GRUB), sem recriar o arquivo
state_set() {
	b=$(printf '# GRUB Environment Block\naic_slot=%s\naic_try=%s\naic_tries=\n' "$1" "$2")
	with_esp rw sh -c '{ printf "%s\n" "$1"; head -c $((1024 - ${#1} - 1)) /dev/zero | tr "\0" "#"; } | dd of="$2" conv=notrunc,fsync 2>/dev/null' _ "$b" "$ENV"
}
