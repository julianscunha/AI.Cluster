#!/bin/sh
# Grava a imagem x86_64 do AI Cluster OS (build/disk-x86.img, de mk-disk.sh) num disco e usa o disco
# inteiro: move a GPT de backup para o fim, sorteia os GUIDs e estende a partição de dados.
#
#   x86/install-disk.sh <imagem[.gz]> <dispositivo> --yes [--env <arquivo>] [--ssh <arquivo>] [--rpc-tls <pasta>] [--efi-entry]
#
# Com .gz (o instalador do pendrive/rede baixa comprimido) a imagem vai descomprimida direto para o disco.
#
# --env grava o arquivo como /worker.env na partição de dados (p4), com debugfs, sem montar.
# --ssh grava o arquivo como /ssh/authorized_keys (dir 700, arquivo 600, root) na partição de dados.
# --rpc-tls grava worker.key, worker.crt e ca.crt da pasta em /rpc-tls (dir 700, chave 600, certificados 644, root):
#   o certificado do túnel RPC que o claim obteve; o worker os lê em /data/rpc-tls.
# --efi-entry cria a entrada de boot "AI Cluster OS" na NVRAM de quem roda o script (só com efibootmgr).
#
# Rode como root num Linux qualquer (um pendrive de recuperação serve), com o disco de destino
# desmontado. APAGA O DISCO INTEIRO. Precisa de: dd, sgdisk (gdisk), e2fsck e resize2fs (e2fsprogs);
# com --env, também debugfs e cmp.
set -eu
[ $# -ge 2 ] || { echo "uso: $0 <imagem> <dispositivo-de-bloco> --yes [--env <arquivo>] [--ssh <arquivo>] [--rpc-tls <pasta>] [--efi-entry]" >&2; exit 2; }
IMG=$1; DEV=$2; shift 2
YES=; ENVF=; SSHF=; RPCD=; EFI=0
while [ $# -gt 0 ]; do
	case $1 in
		--yes) YES=--yes; shift;;
		--env) [ $# -ge 2 ] || { echo "--env exige um arquivo" >&2; exit 2; }; ENVF=$2; shift 2;;
		--ssh) [ $# -ge 2 ] || { echo "--ssh exige um arquivo" >&2; exit 2; }; SSHF=$2; shift 2;;
		--rpc-tls) [ $# -ge 2 ] || { echo "--rpc-tls exige uma pasta" >&2; exit 2; }; RPCD=$2; shift 2;;
		--efi-entry) EFI=1; shift;;
		*) echo "argumento desconhecido: $1" >&2; exit 2;;
	esac
done
[ -f "$IMG" ] && [ -b "$DEV" ] || { echo "uso: $0 <imagem> <dispositivo-de-bloco> --yes [--env <arquivo>] [--ssh <arquivo>] [--rpc-tls <pasta>] [--efi-entry]" >&2; exit 2; }
DEV=$(readlink -f "$DEV")   # symlinks (by-id, mapper) viram o nome real antes das checagens
[ "$(lsblk -dno TYPE "$DEV" 2>/dev/null)" = disk ] || { echo "$DEV não é um disco inteiro (partição, md ou lvm); abortando" >&2; exit 2; }
if [ -n "$(lsblk -nro MOUNTPOINT "$DEV" | tr -d '[:space:]')" ]; then echo "$DEV tem partição montada; abortando" >&2; exit 2; fi
for h in /sys/block/"${DEV##*/}"/holders/* /sys/block/"${DEV##*/}"/*/holders/*; do
	[ -e "$h" ] && { echo "$DEV está em uso (${h##*/}); abortando" >&2; exit 2; }
done
[ "$YES" = --yes ] || { echo "isto apaga $DEV inteiro; confirme com --yes" >&2; exit 2; }
[ -z "$SSHF" ] || [ -f "$SSHF" ] || { echo "--ssh: $SSHF não é um arquivo" >&2; exit 2; }
[ -z "$RPCD" ] || { [ -f "$RPCD/worker.key" ] && [ -f "$RPCD/worker.crt" ] && [ -f "$RPCD/ca.crt" ]; } || { echo "--rpc-tls: $RPCD precisa de worker.key, worker.crt e ca.crt" >&2; exit 2; }
[ -z "$ENVF" ] || [ -f "$ENVF" ] || { echo "--env: $ENVF não é um arquivo" >&2; exit 2; }
for t in dd sgdisk e2fsck resize2fs; do command -v "$t" >/dev/null || { echo "falta $t" >&2; exit 2; }; done
if [ -n "$ENVF$SSHF$RPCD" ]; then for t in debugfs cmp; do command -v "$t" >/dev/null || { echo "falta $t (--env)" >&2; exit 2; }; done; fi
# nunca no disco que está em uso
if grep -q "^$DEV" /proc/mounts || grep -q "^${DEV}p\?[0-9]" /proc/mounts; then echo "$DEV tem partição montada; abortando" >&2; exit 2; fi
# .gz (o instalador baixa comprimido): tamanho sem compactar é o ISIZE do gzip (módulo 4 GiB, a imagem é menor)
case $IMG in *.gz) GZ=1; NEED=$(tail -c 4 "$IMG" | od -An -tu4 | tr -d ' ');; *) GZ=0; NEED=$(stat -c %s "$IMG");; esac
# ISIZE é módulo 4 GiB: se der menos que o próprio .gz, o número está errado (imagem >= 4 GiB); recusa
if [ "$GZ" = 1 ] && [ "$NEED" -lt "$(stat -c %s "$IMG")" ]; then echo "tamanho descompactado de $IMG parece errado (ISIZE, módulo 4 GiB): imagem acima de 4 GiB não é suportada" >&2; exit 2; fi
SIZE=$(blockdev --getsize64 "$DEV")
[ "$SIZE" -ge "$NEED" ] || { echo "disco menor que a imagem" >&2; exit 2; }
[ "$(blockdev --getss "$DEV")" = 512 ] || { echo "setor lógico de $DEV não é 512 B; a imagem não serve para esse disco" >&2; exit 2; }

# worker.env na partição de dados: debugfs escreve no ext4 direto, sem montar; o cmp confere o que ficou
grava_env() {
	debugfs -w -R "rm worker.env" "$1" >/dev/null 2>&1 || true   # regravação: some o da imagem antiga
	debugfs -w -R "write $2 worker.env" "$1" >/dev/null || return 1
	debugfs -w -R "sif worker.env uid 0" "$1" >/dev/null || return 1
	debugfs -w -R "sif worker.env gid 0" "$1" >/dev/null || return 1
	debugfs -w -R "sif worker.env mode 0100600" "$1" >/dev/null || return 1
	debugfs -R "cat worker.env" "$1" 2>/dev/null | cmp -s - "$2"
}
# chaves SSH do operador: /ssh/authorized_keys (a imagem genérica não traz nenhuma)
grava_ssh() {
	debugfs -w -R "mkdir ssh" "$1" >/dev/null 2>&1 || true
	debugfs -w -R "sif ssh uid 0" "$1" >/dev/null || return 1
	debugfs -w -R "sif ssh gid 0" "$1" >/dev/null || return 1
	debugfs -w -R "sif ssh mode 040700" "$1" >/dev/null || return 1
	debugfs -w -R "rm ssh/authorized_keys" "$1" >/dev/null 2>&1 || true
	debugfs -w -R "write $2 ssh/authorized_keys" "$1" >/dev/null || return 1
	debugfs -w -R "sif ssh/authorized_keys uid 0" "$1" >/dev/null || return 1
	debugfs -w -R "sif ssh/authorized_keys gid 0" "$1" >/dev/null || return 1
	debugfs -w -R "sif ssh/authorized_keys mode 0100600" "$1" >/dev/null || return 1
	debugfs -R "cat ssh/authorized_keys" "$1" 2>/dev/null | cmp -s - "$2"
}
# certificado do túnel RPC: /rpc-tls/{worker.key 600, worker.crt 644, ca.crt 644}, dono root; o cmp confere cada um.
# O serviço do worker passa o dono ao usuário dele no boot (o uid não existe aqui).
grava_rpc() {
	debugfs -w -R "mkdir rpc-tls" "$1" >/dev/null 2>&1 || true
	debugfs -w -R "sif rpc-tls uid 0" "$1" >/dev/null || return 1
	debugfs -w -R "sif rpc-tls gid 0" "$1" >/dev/null || return 1
	debugfs -w -R "sif rpc-tls mode 040700" "$1" >/dev/null || return 1
	for par in worker.key:0100600 worker.crt:0100644 ca.crt:0100644; do
		f=${par%%:*}; m=${par##*:}
		debugfs -w -R "rm rpc-tls/$f" "$1" >/dev/null 2>&1 || true
		debugfs -w -R "write $2/$f rpc-tls/$f" "$1" >/dev/null || return 1
		debugfs -w -R "sif rpc-tls/$f uid 0" "$1" >/dev/null || return 1
		debugfs -w -R "sif rpc-tls/$f gid 0" "$1" >/dev/null || return 1
		debugfs -w -R "sif rpc-tls/$f mode $m" "$1" >/dev/null || return 1
		debugfs -R "cat rpc-tls/$f" "$1" 2>/dev/null | cmp -s - "$2/$f" || return 1
	done
}
# entrada de boot na NVRAM desta máquina; sem efibootmgr só avisa (o caminho EFI/BOOT já dá boot)
entrada_efi() {
	command -v efibootmgr >/dev/null || { echo "efibootmgr não existe; entrada de boot não criada" >&2; return 0; }
	# remove entradas antigas com o mesmo nome (reinstalação) e põe a nova em PRIMEIRO na BootOrder:
	# o firmware de alguns notebooks ignora a ordem se a entrada nova não estiver na frente
	for b in $(efibootmgr | sed -n 's/^Boot\([0-9A-Fa-f]\{4\}\)\*\? AI Cluster OS.*/\1/p'); do efibootmgr -b "$b" -B >/dev/null || true; done
	efibootmgr -c -d "$1" -p 1 -l '\EFI\BOOT\BOOTX64.EFI' -L "AI Cluster OS" >/dev/null || return 1
	new=$(efibootmgr | sed -n 's/^Boot\([0-9A-Fa-f]\{4\}\)\*\? AI Cluster OS.*/\1/p' | head -n 1)
	[ -n "$new" ] || return 1
	old=$(efibootmgr | sed -n 's/^BootOrder: *//p' | tr ',' '\n' | grep -vi "^$new$" | paste -sd, -)
	efibootmgr -o "$new${old:+,$old}" >/dev/null
}

# gzip -t antes de tocar no disco: falha de descompressão não pode passar como gravação bem-sucedida
if [ "$GZ" = 1 ]; then gzip -t "$IMG" || { echo "$IMG está corrompido (gzip -t); nada foi gravado" >&2; exit 2; }; fi
if [ "$GZ" = 1 ]; then gzip -dc "$IMG" | dd of="$DEV" bs=4M conv=fsync; else dd if="$IMG" of="$DEV" bs=4M conv=fsync; fi
sgdisk -e "$DEV" >/dev/null                     # GPT de backup no fim do disco
sgdisk -G "$DEV" >/dev/null                     # GUIDs novos: dois discos com a mesma imagem não colidem
# partição de dados (4): mesmo início, até o fim do disco
START=$(sgdisk -i 4 "$DEV" | sed -n 's/^First sector: \([0-9]*\).*/\1/p')
[ -n "$START" ] || { echo "não achei o início da partição de dados; GPT não alterada" >&2; exit 1; }
sgdisk -d 4 -n "4:$START:0" -t 4:8300 -c 4:aic-data "$DEV" >/dev/null
partprobe "$DEV" 2>/dev/null || partx -u "$DEV" 2>/dev/null || blockdev --rereadpt "$DEV" || true
sleep 1
P4=$(ls "${DEV}"4 "${DEV}p4" 2>/dev/null | head -n 1)
[ -b "$P4" ] || { echo "o kernel não enxergou a partição 4 de $DEV; reinicie e rode: e2fsck -f ${DEV}4 && resize2fs ${DEV}4" >&2; exit 1; }
e2fsck -fy "$P4" >/dev/null || [ $? -lt 4 ] || { echo "e2fsck falhou em $P4" >&2; exit 1; }
resize2fs "$P4" >/dev/null
[ -z "$ENVF" ] || grava_env "$P4" "$ENVF" || { echo "não consegui gravar $ENVF como worker.env em $P4" >&2; exit 1; }
[ -z "$SSHF" ] || grava_ssh "$P4" "$SSHF" || { echo "não consegui gravar $SSHF como ssh/authorized_keys em $P4" >&2; exit 1; }
[ -z "$RPCD" ] || grava_rpc "$P4" "$RPCD" || { echo "não consegui gravar o certificado do túnel de $RPCD como rpc-tls em $P4" >&2; exit 1; }
if [ "$EFI" = 1 ]; then entrada_efi "$DEV" || echo "aviso: efibootmgr falhou; crie a entrada de boot à mão" >&2; fi
sync
echo "instalado em $DEV; desligue, remova o pendrive de recuperação e dê boot pelo disco (UEFI, Secure Boot desligado)"
