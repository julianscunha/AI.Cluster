#!/bin/sh
# Liga o boot pela rede num nó Armbian/u-boot (F15 J4/J5): o u-boot tenta baixar kernel, dtb e
# initramfs do servidor TFTP e, se qualquer passo falhar, segue para o boot local de sempre.
#
#   sh install-netboot.sh install <ip-do-servidor> <id-do-nó> <token-de-join> <controller> [ttl]
#   sh install-netboot.sh remove
#
# Roda NO nó, como root. Guarda o original em /boot/boot.cmd.orig e /boot/boot.scr.orig; `remove`
# os restaura e apaga o netboot.env. Tirar só o /boot/netboot.env também desliga o boot pela rede.
# O token fica no cartão local (netboot.env, 0600), nunca na imagem servida.
set -eu
[ "$(id -u)" = 0 ] || { echo "rode como root" >&2; exit 1; }
BOOT=/boot

case "${1:-}" in
remove)
  [ -f "$BOOT/boot.cmd.orig" ] && cp "$BOOT/boot.cmd.orig" "$BOOT/boot.cmd"
  [ -f "$BOOT/boot.scr.orig" ] && cp "$BOOT/boot.scr.orig" "$BOOT/boot.scr"
  rm -f "$BOOT/netboot.env"
  echo "boot pela rede removido; boot local restaurado"
  exit 0
  ;;
install) ;;
*) echo "uso: $0 install <servidor> <nó> <token> <controller> [ttl] | remove" >&2; exit 2 ;;
esac

SERVER=$2; NODE=$3; TOKEN=$4; CONTROLLER=$5; TTL=${6:-0}
# Os valores entram numa linha do u-boot: só caracteres seguros.
for v in "$SERVER" "$NODE" "$TOKEN" "$CONTROLLER" "$TTL"; do
  case "$v" in *[!A-Za-z0-9._:/@=+-]*) echo "caractere não permitido em: $v" >&2; exit 2 ;; esac
done

[ -f "$BOOT/boot.cmd.orig" ] || cp "$BOOT/boot.cmd" "$BOOT/boot.cmd.orig"
[ -f "$BOOT/boot.scr.orig" ] || cp "$BOOT/boot.scr" "$BOOT/boot.scr.orig"

# Parte sempre do original, para rodar de novo não empilhar blocos.
cp "$BOOT/boot.cmd.orig" "$BOOT/boot.cmd.new"
ANCHOR='echo "Boot script loaded from ${devtype}"'
grep -qF "$ANCHOR" "$BOOT/boot.cmd.new" || { echo "boot.cmd sem a âncora esperada" >&2; exit 1; }

cat > /tmp/netboot.block <<'BLOCK'

# --- boot pela rede (F15 J4/J5): qualquer falha segue para o boot local ---
if test -e ${devtype} ${devnum} ${prefix}netboot.env; then
	load ${devtype} ${devnum} ${load_addr} ${prefix}netboot.env
	env import -t ${load_addr} ${filesize}
	echo "netboot: servidor ${nb_server}"
	# Sem autoload o dhcp só configura a rede; com ele tentaria baixar um arquivo do roteador.
	setenv autoload no
	if dhcp; then
		setenv serverip ${nb_server}
		if tftpboot ${kernel_addr_r} netboot/zImage; then
			if tftpboot ${ramdisk_addr_r} netboot/initramfs.cpio.gz; then
				setenv nb_initrd_size ${filesize}
				if tftpboot ${fdt_addr_r} netboot/dtb; then
					setenv bootargs "console=ttyS0,115200 console=tty1 ip=dhcp panic=10 loglevel=4 ${nb_args}"
					echo "netboot: iniciando o sistema da rede"
					bootz ${kernel_addr_r} ${ramdisk_addr_r}:${nb_initrd_size} ${fdt_addr_r}
				fi
			fi
		fi
	fi
	echo "netboot: indisponivel, seguindo com o boot local"
fi
# --- fim do boot pela rede ---
BLOCK

# Insere o bloco logo depois da âncora.
awk -v anchor="$ANCHOR" '
  { print }
  index($0, anchor) { while ((getline line < "/tmp/netboot.block") > 0) print line }
' "$BOOT/boot.cmd.new" > "$BOOT/boot.cmd"
rm -f "$BOOT/boot.cmd.new" /tmp/netboot.block

mkimage -C none -A arm -T script -d "$BOOT/boot.cmd" "$BOOT/boot.scr" >/dev/null

umask 077
cat > "$BOOT/netboot.env" <<ENV
nb_server=$SERVER
nb_args=aic.controller=$CONTROLLER aic.node=$NODE aic.token=$TOKEN aic.ttl=$TTL
ENV
echo "boot pela rede instalado: servidor $SERVER, nó $NODE, prazo ${TTL}s (original em boot.cmd.orig)"
