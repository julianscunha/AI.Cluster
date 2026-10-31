#!/usr/bin/env bash
# Monta o ambiente de instalação do AI Cluster OS x86_64 (ADR-023): um kernel e um initramfs que rodam
# em RAM, sobem a rede, esperam a aprovação do controller e gravam a imagem com install-disk.sh.
# Saída: build/installer/vmlinuz e build/installer/initramfs.gz (em code/os/image/).
#
#   bash code/os/image/x86/build-installer.sh [chave-pública-pinada]
#   AIC_CONTROLLER=http://controller:50052 bash ... (opcional: grava /aic/installer.conf)
#
# Pré-requisitos: o aic-wait estático em musl no volume aic-target-x86 (bash code/scripts/build-x86.sh;
# AIC_WAIT_FILE aponta outro binário) e a chave pública da release (padrão ~/.aic/release.pub, a mesma
# que build-rootfs.sh pina).
# Kernel (linux-lts), busybox, e2fsprogs, efibootmgr, openssl e lsblk vêm do Alpine 3.20 da imagem.
# sgdisk vem do Debian (glibc, com as bibliotecas copiadas): o Alpine não empacota o sgdisk.
# Nada é gravado em disco aqui; é só montagem de arquivos.
set -euo pipefail
cd "$(dirname "$0")/.."
# versão mínima aceita pelo instalador (não aceita imagem mais antiga, replay/downgrade): a deste build
case ${AIC_RELEASE:-} in ''|*[!0-9.]*|.*|*.) echo "AIC_RELEASE é obrigatório: versão numérica (ex.: 1.0) = mínima aceita" >&2; exit 2;; esac
PUB=${1:-${AIC_PUBS:-${AIC_KEYS:-$HOME/.aic}/release.pub}}
[ -f "$PUB" ] || { echo "falta $PUB (rode release-keys.sh ou passe a chave como argumento)" >&2; exit 2; }
HOSTDIR=$(pwd -W 2>/dev/null || pwd)
rm -rf build/installer
mkdir -p build/installer/in
# AIC_WAIT_FILE troca o binário (teste sem compilar o musl); o padrão é o do volume do build-x86.sh
if [ -n "${AIC_WAIT_FILE:-}" ]; then cp "$AIC_WAIT_FILE" build/installer/in/aic-wait-x86
else
  MSYS_NO_PATHCONV=1 docker run --rm -v aic-target-x86:/t -v "$HOSTDIR/build/installer/in:/out" alpine:3.20 \
    cp /t/x86_64-unknown-linux-musl/release/aic-wait /out/aic-wait-x86 \
    || { echo "falta o aic-wait no volume aic-target-x86 (rode bash code/scripts/build-x86.sh)" >&2; exit 2; }
fi
cp "$PUB" build/installer/in/release.pub
cp x86/aic-install.sh x86/install-disk.sh build/installer/in/

# sgdisk (gptfdisk) do Debian e as bibliotecas dele, em build/installer/glibc, nos mesmos caminhos
MSYS_NO_PATHCONV=1 docker run --rm --platform linux/amd64 -v "$HOSTDIR/build/installer:/o" debian:12-slim sh -c '
  set -e
  apt-get update -qq >/dev/null && apt-get install -y -qq gdisk >/dev/null
  S=$(command -v sgdisk)
  mkdir -p /o/glibc$(dirname "$S") && cp "$S" /o/glibc"$S"
  for lib in $(ldd "$S" | grep -oE "/[^ ]+"); do mkdir -p /o/glibc$(dirname "$lib") && cp -L "$lib" /o/glibc"$lib"; done
'

MSYS_NO_PATHCONV=1 docker run --rm --platform linux/amd64 \
  -e AIC_CONTROLLER="${AIC_CONTROLLER:-}" -e AIC_RELEASE="$AIC_RELEASE" -v "$HOSTDIR/build/installer:/o" alpine:3.20 sh -c '
set -eu
apk add -q --no-cache linux-lts busybox-static kmod cpio util-linux e2fsprogs e2fsprogs-extra efibootmgr openssl >/dev/null
R=/tmp/root
rm -rf "$R"; mkdir -p "$R"/bin "$R"/sbin "$R"/usr/bin "$R"/usr/sbin "$R"/etc/aic "$R"/aic "$R"/proc "$R"/sys "$R"/dev \
  "$R"/run "$R"/tmp "$R"/root "$R"/lib/modules "$R"/var

# bibliotecas que o ldd lista, nos mesmos caminhos (musl: ld-musl e as libs do Alpine)
libs() {
  ldd "$1" 2>/dev/null | grep -oE "/[^ ]+" | sort -u | while read -r l; do
    [ -e "$l" ] || continue
    mkdir -p "$R$(dirname "$l")"; cp -L "$l" "$R$l"
  done
}
put() { # put <binário-do-Alpine> <caminho-na-imagem>
  mkdir -p "$R$(dirname "$2")"; cp -L "$1" "$R$2"; chmod 755 "$R$2"; libs "$1"
}

# busybox estático (ou dinâmico, se o Alpine não tiver o estático) e um link por applet
BB=/bin/busybox.static; [ -f "$BB" ] || BB=/bin/busybox
put "$BB" /bin/busybox
for applet in $("$R/bin/busybox" --list); do
  [ -e "$R/bin/$applet" ] || [ -e "$R/sbin/$applet" ] || ln -s busybox "$R/bin/$applet"
done
[ -e "$R/bin/blkid" ] || { echo "busybox sem blkid: o instalador não confere o pendrive sem ele" >&2; exit 1; }
# ferramentas que o busybox não tem (install-disk.sh usa lsblk; o resto é o que o script chama)
for t in /bin/lsblk /sbin/blockdev /sbin/e2fsck /usr/sbin/debugfs /usr/sbin/resize2fs /usr/sbin/efibootmgr /usr/bin/openssl; do
  put "$t" "$t"
done
# sgdisk do Debian (glibc), com o ld-linux e as bibliotecas que ele pede
cp -a /o/glibc/. "$R"/
chmod 755 "$R/usr/sbin/sgdisk"

# módulos: a lista de quem as interfaces de rede, disco e firmware precisam, com as dependências na ordem
# de carga (o modprobe -D dá a cadeia; o insmod carrega cada arquivo, já descomprimido)
K=$(ls /lib/modules | head -n 1)
for m in virtio_net e1000 e1000e igc igb r8169 realtek tg3 ixgbe nvme ahci ata_piix sd_mod usb_storage uas \
         xhci_pci xhci_hcd ehci_pci ehci_hcd ohci_pci uhci_hcd virtio_pci virtio_blk efivarfs; do
  modprobe -S "$K" -D "$m" 2>/dev/null
done | sed -n "s|^insmod ||p" | awk "!seen[\$0]++" > /tmp/modulos.txt
: > "$R/etc/aic/modules"
while read -r p; do
  dst="$R${p%.gz}"
  mkdir -p "$(dirname "$dst")"
  case $p in *.gz) zcat "$p" > "$dst";; *) cp "$p" "$dst";; esac
  echo "${p%.gz}" >> "$R/etc/aic/modules"
done < /tmp/modulos.txt

# o que o instalador lê e roda
cp /o/in/aic-install.sh "$R/init"; chmod 755 "$R/init"
cp /o/in/install-disk.sh "$R/usr/sbin/install-disk.sh"; chmod 755 "$R/usr/sbin/install-disk.sh"
cp /o/in/aic-wait-x86 "$R/usr/bin/aic-wait"; chmod 755 "$R/usr/bin/aic-wait"
cp /o/in/release.pub "$R/etc/aic/release.pub"; chmod 644 "$R/etc/aic/release.pub"
echo "$AIC_RELEASE" > "$R/etc/aic/min-version"
cat > "$R/etc/aic/udhcpc.sh" <<UDHCPC
#!/bin/sh
# busybox udhcpc: aplica o aluguel (bound/renew) e limpa a placa (deconfig)
case \$1 in
bound|renew)
	ifconfig "\$interface" "\$ip" netmask "\$subnet" up
	route del default 2>/dev/null
	for r in \$router; do route add default gw "\$r" dev "\$interface" 2>/dev/null; done
	: > /etc/resolv.conf
	for d in \$dns; do echo "nameserver \$d" >> /etc/resolv.conf; done
	echo "\$ip" > "/run/aic-ip-\$interface"
	;;
deconfig) ifconfig "\$interface" up ;;
esac
UDHCPC
chmod 755 "$R/etc/aic/udhcpc.sh"
if [ -n "${AIC_CONTROLLER:-}" ]; then echo "controller=$AIC_CONTROLLER" > "$R/aic/installer.conf"; else echo "# controller= (ou aic.controller= na linha do kernel)" > "$R/aic/installer.conf"; fi

cp /boot/vmlinuz-lts /o/vmlinuz
cd "$R" && find . -print | cpio -o -H newc --owner=0:0 2>/dev/null | gzip -9 > /o/initramfs.gz
echo "modulos: $(wc -l < /tmp/modulos.txt)  kernel: $K"
ls -l /o/vmlinuz /o/initramfs.gz
'
rm -rf build/installer/in build/installer/glibc
ls -l build/installer
