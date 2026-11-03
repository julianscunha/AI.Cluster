#!/bin/sh
# Estado da rede no console (aic-console): cabo e Wi-Fi, com sysfs, `ip` e aic-wifi.status simulados.
set -eu
T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
F=$(dirname "$0")/../os/image/overlay/usr/sbin/aic-console
mkdir -p "$T/sys/lo" "$T/sys/eth0" "$T/sys/wlan0/wireless" "$T/sys/dummy0" "$T/bin"
# interface fisica tem o link `device` no sysfs; a virtual (dummy0 do Pi) nao tem e nao conta como cabo
mkdir -p "$T/dev"; ln -s "$T/dev" "$T/sys/eth0/device"; ln -s "$T/dev" "$T/sys/wlan0/device"
C_OK=OK C_WARN=WARN C_BAD=BAD
# `ip -4 -o addr show IF` simulado: o IP vem de $T/ip.IF
cat > "$T/bin/ip" <<'IP'
#!/bin/sh
f="$IPDIR/ip.$5"; [ -s "$f" ] && echo "2: $5    inet $(cat "$f")/24 brd x scope global $5"; exit 0
IP
chmod +x "$T/bin/ip"
sed -n '/inicio-rede/,/fim-rede/p' "$F" > "$T/bloco.sh"
run() { PATH="$T/bin:$PATH" IPDIR="$T" AIC_SYSNET="$T/sys" AIC_WIFI_STATUS="$T/wifi.status" C_OK=$C_OK C_WARN=$C_WARN C_BAD=$C_BAD sh -c ". $T/bloco.sh; echo \"\$dot_net|\$net\"" ; }
ck() { [ "$1" = "$2" ] || { echo "FALHOU: esperado [$2] veio [$1]"; exit 1; }; }
# sem rede nenhuma: cabo solto, wifi sem config
echo 0 > "$T/sys/eth0/carrier"
ck "$(run)" "BAD|cabo: sem link (cabo solto?)  |  wifi: nao configurado"
# link ok sem IP
echo 1 > "$T/sys/eth0/carrier"
ck "$(run)" "WARN|cabo: link ok, sem IP (DHCP)  |  wifi: nao configurado"
# cabo com IP
echo 192.168.1.18 > "$T/ip.eth0"
ck "$(run)" "OK|cabo: 192.168.1.18  |  wifi: nao configurado"
# cabo com IP e wifi conectando: verde (o cabo tem IP)
echo "Wi-Fi: conectando" > "$T/wifi.status"
ck "$(run)" "OK|cabo: 192.168.1.18  |  wifi: conectando"
# sem cabo, wifi conectando: amarelo
echo 0 > "$T/sys/eth0/carrier"; rm -f "$T/ip.eth0"
ck "$(run)" "WARN|cabo: sem link (cabo solto?)  |  wifi: conectando"
# sem cabo, wifi com IP: verde
echo 192.168.1.40 > "$T/ip.wlan0"; echo "Wi-Fi: conectado a JuliansNET, IP 192.168.1.40" > "$T/wifi.status"
ck "$(run)" "OK|cabo: sem link (cabo solto?)  |  wifi: conectado a JuliansNET, IP 192.168.1.40"
echo "OK: console/rede"
