#!/usr/bin/env bash
# Wi-Fi do AI Cluster OS (ADR-030): testa sem hardware o script `wifi` (credencial) e o `netwatch` (vigia) do overlay,
# com sysfs e comandos simulados. Roda no Git Bash/Linux: bash tests/os-wifi-overlay.sh
set -uo pipefail
O=$(cd "$(dirname "$0")/../os/image/overlay" && pwd)
T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
fail() { echo "FALHA: $1"; exit 1; }
for f in wifi netwatch; do sh -n "$O/usr/libexec/aic/$f" || fail "sh -n $f"; done
sh -n "$O/etc/init.d/aic-wifi" || fail "sh -n aic-wifi"
export AIC_WIFI_DIR=$T/wifi AIC_WIFI_STATUS=$T/status
W=$O/usr/libexec/aic/wifi

# senha curta e SSID invalido são recusados e nada é gravado
printf 'curta\n' | sh "$W" set Casa 2>/dev/null && fail "aceitou senha curta"
printf 'senha-boa-123\n' | sh "$W" set "" 2>/dev/null && fail "aceitou SSID vazio"
printf 'senha-boa-123\n' | sh "$W" set Casa --country br 2>/dev/null && fail "aceitou pais invalido"
[ ! -e "$AIC_WIFI_DIR/wpa_supplicant.conf" ] || fail "gravou com entrada invalida"

SEGREDO='s3gredo"com aspas #1'
OUT=$(printf '%s\n' "$SEGREDO" | sh "$W" set "Rede Casa" --country BR 2>&1) || fail "set falhou: $OUT"
case "$OUT" in *s3gredo*) fail "a senha apareceu na saida" ;; esac
C=$AIC_WIFI_DIR/wpa_supplicant.conf
[ -f "$C" ] || fail "sem conf"
case "$(uname -s)" in MINGW*|MSYS*) ;; *) [ "$(stat -c %a "$C")" = 600 ] || fail "conf não é 0600" ;; esac # NTFS não tem modo
grep -q '^country=BR$' "$C" && grep -q 'key_mgmt=WPA-PSK SAE' "$C" && grep -q 'ssid=5265646520436173610*' "$C" || fail "conf sem os campos esperados"
ST=$(sh "$W" status); case "$ST" in *'Rede Casa'*) ;; *) fail "status não mostra o SSID" ;; esac
case "$ST" in *s3gredo*) fail "status vazou a senha" ;; esac
sh "$W" clear >/dev/null; [ ! -e "$C" ] || fail "clear não removeu"

# netwatch: stubs de ip/ifup/ifdown/wpa_supplicant/wpa_cli/udhcpc; cabo com portadora e sem IP -> ifup eth0; wlan0 nunca passa por ifup/ifdown
mkdir -p "$T/bin" "$T/sys/eth0" "$T/run"; echo 1 > "$T/sys/eth0/carrier"
printf '#!/bin/sh\nexit 0\n' > "$T/bin/ifdown"
printf '#!/bin/sh\necho "$@" >> %s/calls\n' "$T" > "$T/bin/ifup"
# ip: so devolve endereco para wlan0 quando $T/wlan0.ip existe
printf '#!/bin/sh\ncase "$*" in *wlan0*) [ -f %s/wlan0.ip ] && cat %s/wlan0.ip ;; esac\nexit 0\n' "$T" "$T" > "$T/bin/ip"
# wpa_supplicant: registra os argumentos e fica vivo (como o real sem -B, que NAO grava pidfile); wpa_cli le o estado de $T/wpa.state
printf '#!/bin/sh\necho "$@" >> %s/wpa.calls\nexec sleep 60\n' "$T" > "$T/bin/wpa_supplicant"
printf '#!/bin/sh\nfor a in "$@"; do case $a in status) cat %s/wpa.state;; scan_results) cat %s/wpa.scan 2>/dev/null;; esac; done\n' "$T" "$T" > "$T/bin/wpa_cli"
printf '#!/bin/sh\necho "udhcpc $IF_METRIC $*" >> %s/dhcp.calls\n' "$T" > "$T/bin/udhcpc"
chmod +x "$T/bin/"*
export AIC_SYS_NET=$T/sys AIC_WIFI_CONF=$T/wifi/wpa_supplicant.conf AIC_RUN=$T/run
NW() { PATH=$T/bin:$PATH sh "$O/usr/libexec/aic/netwatch" once; }
NW
grep -qx eth0 "$T/calls" || fail "não refez o DHCP do cabo"
mkdir -p "$T/wifi"; printf '# aic-ssid: Casa\n' > "$AIC_WIFI_CONF"
NW
grep -q 'sem interface wlan0' "$AIC_WIFI_STATUS" || fail "não avisou adaptador ausente"
mkdir -p "$T/sys/wlan0"; : > "$T/calls"
NW
grep -q wlan0 "$T/calls" && fail "netwatch fez ifup/ifdown do wlan0 (interrompe a associação)"
[ "$(wc -l < "$T/wpa.calls")" = 1 ] || fail "deveria iniciar um wpa_supplicant"
grep -q -- '-i wlan0 -c .*wpa_supplicant.conf' "$T/wpa.calls" || fail "argumentos do wpa_supplicant"
grep -q -- ' -P ' "$T/wpa.calls" && fail "sem -B o -P nao grava pidfile: o netwatch grava o pid"
grep -q conectando "$AIC_WIFI_STATUS" || fail "status deveria ser conectando"
# varias passadas sem associar: não recicla nem reinicia o wpa_supplicant, e não sobe o DHCP
echo wpa_state=SCANNING > "$T/wpa.state"
for i in 1 2 3; do NW; done
[ "$(wc -l < "$T/wpa.calls")" = 1 ] || fail "reiniciou o wpa_supplicant vivo"
[ ! -e "$T/dhcp.calls" ] || fail "udhcpc antes de associar"
# motivos
printf 'aa:bb\t2412\t-60\t[WPA2]\tOutra\n' > "$T/wpa.scan"; NW
grep -q 'sem sinal de Casa' "$AIC_WIFI_STATUS" || fail "motivo: sem sinal"
printf 'aa:bb\t2412\t-60\t[WPA2]\tCasa\n' > "$T/wpa.scan"
echo 'wlan0: CTRL-EVENT-SSID-TEMP-DISABLED id=0 ssid="Casa" auth_failures=2 duration=20 reason=WRONG_KEY' > "$T/run/wpa_supplicant.wlan0.log"; NW
grep -q 'senha/PSK errada' "$AIC_WIFI_STATUS" || fail "motivo: PSK errada"
echo 'wlan0: CTRL-EVENT-AUTH-REJECT e0:d3:62:00:96:18 auth_type=3 auth_transaction=2 status_code=1' > "$T/run/wpa_supplicant.wlan0.log"; NW
grep -q 'autenticacao negada.*status_code=1' "$AIC_WIFI_STATUS" || fail "motivo: autenticação negada"
# associado: so entao o udhcpc do wlan0, com pidfile proprio e metrica acima do cabo
printf 'wpa_state=COMPLETED\nssid=Casa\n' > "$T/wpa.state"; NW
grep -qx 'udhcpc 1000 -b -R -p .*/udhcpc.wlan0.pid -i wlan0' "$T/dhcp.calls" || fail "udhcpc do wlan0"
grep -q 'obtendo IP' "$AIC_WIFI_STATUS" || fail "status: obtendo IP"
echo '    inet 192.168.1.50/24 brd 192.168.1.255 scope global wlan0' > "$T/wlan0.ip"; NW
grep -qx 'Wi-Fi: conectado a Casa, IP 192.168.1.50' "$AIC_WIFI_STATUS" || fail "status: conectado"
# wpa_supplicant morreu: reinicia
kill "$(cat "$T/run/wpa_supplicant.wlan0.pid")"; sleep 0.2; NW
[ "$(wc -l < "$T/wpa.calls")" = 2 ] || fail "não reiniciou o wpa_supplicant morto"
kill "$(cat "$T/run/wpa_supplicant.wlan0.pid")" 2>/dev/null
# aic-wifi stop: stop_post mata so o wpa_supplicant e o udhcpc do wlan0 (pelos pidfiles), nao outro processo
sleep 30 & WP=$!; sleep 30 & UP=$!; sleep 30 & OTHER=$!
echo $WP > "$T/run/wpa_supplicant.wlan0.pid"; echo $UP > "$T/run/udhcpc.wlan0.pid"
eval "$(sed -n '/^stop_post()/,/^}/p' "$O/etc/init.d/aic-wifi" | sed "s#/run/#$T/run/#g")"
stop_post
kill -0 $WP 2>/dev/null && fail "stop_post nao parou o wpa_supplicant"
kill -0 $UP 2>/dev/null && fail "stop_post nao parou o udhcpc"
kill -0 $OTHER 2>/dev/null || fail "stop_post matou processo fora do pidfile"
[ ! -e "$T/run/wpa_supplicant.wlan0.pid" ] || fail "stop_post deixou o pidfile"
kill $OTHER 2>/dev/null; true
# imagem: wlan0 manual (sem wpa-conf) e o init para só os processos do wlan0
grep -q '^iface wlan0 inet manual' "$O/etc/network/interfaces" && ! grep -q wpa-conf "$O/etc/network/interfaces" || fail "interfaces do wlan0"
grep -q 'stop_post' "$O/etc/init.d/aic-wifi" && ! grep -Eq 'killall|pkill' "$O/etc/init.d/aic-wifi" "$O/usr/libexec/aic/netwatch" || fail "init/netwatch com kill amplo"

# driver: ausente -> presente -> carregado
export AIC_SYS_MODULE=$T/mod AIC_MODULES_DIR=$T/lib
case "$(sh "$W" status)" in *'ath9k_htc: ausente'*) ;; *) fail "driver deveria estar ausente" ;; esac
mkdir -p "$T/lib/4.19.20-sunxi"; : > "$T/lib/4.19.20-sunxi/ath9k_htc.ko"
case "$(sh "$W" status)" in *'presente, nao carregado'*) ;; *) fail "driver deveria estar presente" ;; esac
mkdir -p "$T/mod/ath9k_htc"
case "$(sh "$W" status)" in *'ath9k_htc: carregado'*) ;; *) fail "driver deveria estar carregado" ;; esac
# imagem: o Dockerfile copia a saída do build, roda depmod e carrega o módulo no boot
D=$(dirname "$O")
grep -q 'COPY wifi-lib/ /lib/' "$D/Dockerfile" && grep -q 'depmod -b / 4.19.20-sunxi' "$D/Dockerfile" && grep -q 'ath9k_htc >> /etc/modules' "$D/Dockerfile" || fail "Dockerfile sem módulos Wi-Fi"
grep -q "^RUN if .*printf '%s\\\\n' ghash-generic gcm ccm ath9k_htc >> /etc/modules; fi$" "$D/Dockerfile" || fail "ccm/gcm precisam carregar antes do ath9k_htc (sem eles o set_key da PTK dá ENOENT)"
grep -q "RESOLV_CONF=.*run/resolv.conf" "$D/Dockerfile" || fail "udhcpc do wlan0 precisa do RESOLV_CONF em /run"
grep -q "ignore_routes_with_linkdown = 1" "$O/etc/sysctl.d/10-aic-network.conf" || fail "sem ignore_routes_with_linkdown o trafego segue pelo eth0 sem cabo e o Wi-Fi nao assume"
grep -q 'wifi-lib' "$D/build-rootfs.sh" || fail "build-rootfs.sh não prepara wifi-lib"
grep -q "modules.dep" "$D/Dockerfile" || fail "Dockerfile sem modules.dep explicito (modpost deixa cfg80211/mac80211 sem dependencias)"
echo "OK: wifi/netwatch"
