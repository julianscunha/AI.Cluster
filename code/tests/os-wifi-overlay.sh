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
sh "$W" status | grep -q 'Rede Casa' || fail "status não mostra o SSID"
sh "$W" status | grep -q s3gredo && fail "status vazou a senha"
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
echo "OK: wifi/netwatch"
