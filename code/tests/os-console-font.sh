#!/usr/bin/env bash
# Fonte do console do AI Cluster OS: testa sem hardware a escolha em `tty_font` (ui.sh) com framebuffer
# e setfont simulados. Roda no Git Bash/Linux: bash tests/os-console-font.sh
set -uo pipefail
O=$(cd "$(dirname "$0")/../os/image/overlay" && pwd)
T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
fail() { echo "FALHA: $1"; exit 1; }
for f in usr/lib/aic/ui.sh usr/sbin/aic-console usr/sbin/aic-splash-run; do sh -n "$O/$f" || fail "sh -n $f"; done
mkdir -p "$T/fonts"; for n in 16 20 24 32; do : > "$T/fonts/ter-v${n}n.psf.gz"; done
printf '#!/bin/sh\necho "$@" > %s/args\n' "$T" > "$T/setfont"; chmod +x "$T/setfont"
export AIC_FONT_DIR=$T/fonts AIC_SETFONT=$T/setfont AIC_FONT_MARK=$T/mark AIC_FB_SIZE=$T/fb
escolhe() { # largura,altura -> altura da fonte escolhida
	rm -f "$T/mark" "$T/args"; echo "$1" > "$T/fb"
	sh -c ". '$O/usr/lib/aic/ui.sh'; tty_font"
}
[ "$(escolhe 1920,1080)" = 32 ] || fail "1920x1080 deveria usar 32"
grep -q 'ter-v32n' "$T/args" || fail "setfont não recebeu a fonte 32"
[ -e "$T/mark" ] || fail "sem marca"
rm -f "$T/args"; sh -c ". '$O/usr/lib/aic/ui.sh'; tty_font"; [ ! -e "$T/args" ] || fail "trocou a fonte duas vezes"
[ "$(escolhe 1366,768)" = 24 ] || fail "1366x768 deveria usar 24"
[ "$(escolhe 1024,768)" = 24 ] || fail "1024x768 deveria usar 24"
[ "$(escolhe 800,600)" = 20 ] || fail "800x600 deveria usar 20 (80 colunas de 10 px)"
[ "$(escolhe 640,480)" = 16 ] || fail "640x480 deveria usar 16"
[ -z "$(escolhe 320,200)" ] || fail "tela pequena não deveria trocar"
rm -f "$T/mark"; rm -f "$T/fb"; [ -z "$(sh -c ". '$O/usr/lib/aic/ui.sh'; tty_font")" ] || fail "sem framebuffer não deveria trocar"
echo "OK: fonte do console"
