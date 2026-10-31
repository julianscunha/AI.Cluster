#!/bin/sh
# Monta o sistema mínimo do worker para o boot pela rede (F15 J4/J5): um initramfs com busybox,
# o worker e um servidor SSH (dropbear), que roda inteiro na RAM do worker.
#
#   sh build-initramfs.sh <ai-cluster-worker> <diretório-de-saída>
#
# Roda NA máquina de destino (mesma arquitetura e mesma glibc do worker que vai bootar), como root,
# num Debian/Ubuntu com apt: ela copia de si mesma o busybox, o dropbear e as bibliotecas.
# Saída: <diretório>/initramfs.cpio.gz. A senha do root do SSH é sorteada a cada build e impressa
# no fim (SSH_PASSWORD=... para fixá-la). A imagem NÃO leva o token de join: ele chega pela linha
# de comando do kernel (aic.token=), que vem do cartão local do worker.
#
# O init lê da linha de comando: aic.controller= aic.node= aic.token= aic.ttl=SEGUNDOS (reinicia o
# worker depois desse tempo; 0 ou ausente = nunca). É o que faz do boot pela rede um slot em teste:
# passado o prazo o worker volta ao u-boot e, sem servidor TFTP, sobe do cartão.
set -eu

WORKER=$1
OUT=$2
[ -f "$WORKER" ] || { echo "binário não encontrado: $WORKER" >&2; exit 2; }
[ "$(id -u)" = 0 ] || { echo "rode como root" >&2; exit 1; }
mkdir -p "$OUT"
# O ldd só lista as bibliotecas de um binário executável.
chmod +x "$WORKER"

for pkg in busybox-static dropbear-bin; do
  dpkg -s "$pkg" >/dev/null 2>&1 || apt-get install -y -qq "$pkg" >/dev/null
done

R=$(mktemp -d)
trap 'rm -rf "$R"' EXIT
mkdir -p "$R/bin" "$R/sbin" "$R/usr/bin" "$R/etc/dropbear" "$R/proc" "$R/sys" "$R/dev" \
  "$R/tmp" "$R/var/tmp" "$R/root" "$R/opt" "$R/lib"

# busybox e um link por applet
cp /bin/busybox "$R/bin/busybox"
for applet in $(/bin/busybox --list); do
  [ -e "$R/bin/$applet" ] || ln -s busybox "$R/bin/$applet"
done

# Um binário dinâmico leva consigo as bibliotecas que o ldd lista (caminho absoluto, seguindo links).
with_libs() { # with_libs <binário> <destino-dentro-da-imagem>
  mkdir -p "$R$(dirname "$2")"
  cp "$1" "$R$2"
  ldd "$1" | grep -oE '/[^ ]+' | while read -r lib; do
    [ -e "$lib" ] || continue
    mkdir -p "$R$(dirname "$lib")"
    cp -L "$lib" "$R$lib"
  done
}
with_libs "$WORKER" /opt/ai-cluster-worker
chmod 755 "$R/opt/ai-cluster-worker"  # o arquivo pode chegar sem o bit de execução (sftp)
with_libs "$(command -v dropbear)" /usr/bin/dropbear
with_libs "$(command -v dropbearkey)" /usr/bin/dropbearkey
# crypt() e a resolução de nomes procuram estas em tempo de execução, o ldd não as vê.
for lib in /lib/arm-linux-gnueabihf/libnss_files.so.2 /lib/arm-linux-gnueabihf/libnss_dns.so.2 \
  /lib/arm-linux-gnueabihf/libresolv.so.2 /lib/arm-linux-gnueabihf/libcrypt.so.1; do
  [ -e "$lib" ] && { mkdir -p "$R$(dirname "$lib")"; cp -L "$lib" "$R$lib"; }
done

# Chave de host estável por imagem (o ssh do operador não reclama a cada boot) e senha sorteada.
dropbearkey -t ecdsa -s 256 -f "$R/etc/dropbear/key" >/dev/null 2>&1
SSH_PASSWORD=${SSH_PASSWORD:-$(head -c 18 /dev/urandom | base64 | tr -d '/+=' | cut -c1-16)}
HASH=$(printf '%s' "$SSH_PASSWORD" | openssl passwd -6 -stdin)
echo 'root:x:0:0:root:/root:/bin/sh' > "$R/etc/passwd"
echo "root:$HASH:19000:0:99999:7:::" > "$R/etc/shadow"
chmod 600 "$R/etc/shadow"
echo 'root:x:0:' > "$R/etc/group"
echo '/bin/sh' > "$R/etc/shells"
printf 'passwd: files\ngroup: files\nshadow: files\nhosts: files dns\n' > "$R/etc/nsswitch.conf"
echo '127.0.0.1 localhost' > "$R/etc/hosts"

cat > "$R/init" <<'INIT'
#!/bin/sh
export PATH=/bin:/sbin:/usr/bin HOME=/root
mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t devtmpfs devtmpfs /dev 2>/dev/null
mkdir -p /dev/pts
mount -t devpts devpts /dev/pts
mount -t tmpfs tmpfs /tmp
# O kernel já configurou a rede (ip=dhcp) e escreveu o DNS aqui.
ln -sf /proc/net/pnp /etc/resolv.conf

arg() { for word in $(cat /proc/cmdline); do case $word in aic.$1=*) echo "${word#aic.$1=}" ;; esac; done; }
CONTROLLER=$(arg controller); NODE=$(arg node); TOKEN=$(arg token); TTL=$(arg ttl)
[ -n "$NODE" ] && hostname "$NODE"
IP=$(ip -4 -o addr show eth0 2>/dev/null | awk '{print $4}' | head -n 1)
echo "== AI.Cluster - AI Cluster OS (recuperacao) =="
echo "== AI Cluster netboot: no=$NODE ip=$IP controller=$CONTROLLER ttl=${TTL:-0}"

dropbear -r /etc/dropbear/key -p 22 -E 2>/tmp/dropbear.log &

case "$CONTROLLER" in http://*) export AI_CLUSTER_INSECURE=1 ;; esac
# aic.token é o token de RECUPERAÇÃO (só registra e dá heartbeat), nunca o de join; o worker só aparece
export AI_CLUSTER_CONTROLLER="$CONTROLLER" AI_CLUSTER_JOIN_TOKEN="$TOKEN" AI_CLUSTER_NODE_ID="$NODE" AI_CLUSTER_PRESENCE_ONLY=1
cd /tmp
/opt/ai-cluster-worker >/tmp/worker.log 2>&1 &

if [ "${TTL:-0}" -gt 0 ] 2>/dev/null; then
  ( sleep "$TTL"; echo "== prazo de ${TTL}s vencido; reiniciando"; sync; reboot -f ) &
fi
# O init não pode terminar: o kernel entraria em pânico.
while :; do sleep 3600; done
INIT
chmod 755 "$R/init"

( cd "$R" && find . | cpio -o -H newc --owner=0:0 2>/dev/null | gzip -9 ) > "$OUT/initramfs.cpio.gz"
ls -l "$OUT/initramfs.cpio.gz"
echo "SSH_PASSWORD=$SSH_PASSWORD"
