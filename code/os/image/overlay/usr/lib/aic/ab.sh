#!/bin/sh
# A/B no armv7 (u-boot): os slots são as partições 2 (A) e 3 (B) do cartão e o estado fica no setor cru
# 2016 (o u-boot não escreve na FAT). Funções comuns a todas as plataformas:
#   slot_dev A|B           caminho da partição do slot
#   state_get <variável>   valor atual (aic_slot, aic_try, aic_tries)
#   state_set <slot> <try> grava o slot confirmado e o slot em teste (vazio = nenhum)
slot_dev() { case "$1" in A) echo /dev/mmcblk0p2;; B) echo /dev/mmcblk0p3;; *) return 1;; esac; }
state_get() { dd if=/dev/mmcblk0 bs=512 skip=2016 count=1 2>/dev/null | tr -d '\0' | sed -n "s/^$1=//p"; }
# conv=sync completa o setor com zeros
state_set() { printf 'aic_slot=%s\naic_try=%s\naic_tries=\naic_boots=\n' "$1" "$2" | dd of=/dev/mmcblk0 bs=512 seek=2016 count=1 conv=sync,fsync 2>/dev/null; }
cur_slot() { sed -n 's/.*aic\.slot=\([AB]\).*/\1/p' /proc/cmdline | grep .; }
# no armv7 o commit sempre regrava (zera também a contagem de boots da recuperação por rede)
state_confirm() { state_set "$1" ""; }
