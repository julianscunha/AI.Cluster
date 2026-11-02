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

# netwatch: stubs de ip/ifup/ifdown; cabo com portadora e sem IP → ifup eth0; wlan0 ausente com credencial → status
mkdir -p "$T/bin" "$T/sys/eth0"; echo 1 > "$T/sys/eth0/carrier"
printf '#!/bin/sh\nexit 0\n' > "$T/bin/ifdown"
printf '#!/bin/sh\necho "$@" >> %s/calls\n' "$T" > "$T/bin/ifup"
printf '#!/bin/sh\nexit 0\n' > "$T/bin/ip"
chmod +x "$T/bin/"*
export AIC_SYS_NET=$T/sys AIC_WIFI_CONF=$T/wifi/wpa_supplicant.conf
PATH=$T/bin:$PATH sh "$O/usr/libexec/aic/netwatch" once
grep -qx eth0 "$T/calls" || fail "não refez o DHCP do cabo"
mkdir -p "$T/wifi"; : > "$AIC_WIFI_CONF"
PATH=$T/bin:$PATH sh "$O/usr/libexec/aic/netwatch" once
grep -q 'sem interface wlan0' "$AIC_WIFI_STATUS" || fail "não avisou adaptador ausente"
mkdir -p "$T/sys/wlan0"; : > "$T/calls"
PATH=$T/bin:$PATH sh "$O/usr/libexec/aic/netwatch" once
grep -qx wlan0 "$T/calls" || fail "não subiu o wlan0"

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
grep -q 'wifi-lib' "$D/build-rootfs.sh" || fail "build-rootfs.sh não prepara wifi-lib"
grep -q "modules.dep" "$D/Dockerfile" || fail "Dockerfile sem modules.dep explicito (modpost deixa cfg80211/mac80211 sem dependencias)"
echo "OK: wifi/netwatch"
