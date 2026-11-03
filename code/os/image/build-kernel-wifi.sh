#!/usr/bin/env bash
# Compila os módulos Wi-Fi do TP-Link TL-WN821N (ath9k_htc) para o kernel 4.19.20-sunxi do Orange Pi PC,
# sem trocar o zImage (ADR-030). Saída em .tmp/kernel-wifi/out/: lib/modules/4.19.20-sunxi/**.ko e lib/firmware/htc_7010.fw.
#
#   bash code/os/image/build-kernel-wifi.sh
#
# O .config é o do próprio zImage (IKCONFIG embutido; sai de build/zImage com extract-ikconfig em Python).
# Fonte: linux-4.19.20 do kernel.org (sem os patches do Armbian: não há tag do Armbian dessa época);
# compilador: gcc 7 do Ubuntu 18.04 (o zImage usa Linaro 7.2). MODVERSIONS está desligado no .config, então o
# que vale é o vermagic: "4.19.20-sunxi SMP mod_unload ARMv7 thumb2 p2v8". Nada é montado nem gravado em cartão.
set -euo pipefail
cd "$(dirname "$0")"
OUT=$(cd ../../.. && pwd)/.tmp/kernel-wifi; mkdir -p "$OUT/out"
if [ ! -f "$OUT/config" ]; then
  python -I - "$(pwd)/build/zImage" "$OUT/config" <<'PY'
import sys, zlib, re
d = open(sys.argv[1], 'rb').read()
for m in re.finditer(b'\x1f\x8b\x08', d):
    try: raw = zlib.decompressobj(31).decompress(d[m.start():])
    except Exception: continue
    if len(raw) > 1000000: break
i = raw.find(b'IKCFG_ST')
open(sys.argv[2], 'wb').write(zlib.decompressobj(31).decompress(raw[i + 8:]))
PY
fi
V=4.19.20
MSYS_NO_PATHCONV=1 docker run --rm -v aic-kernel-wifi:/src -v "$(cd "$OUT" && pwd -W 2>/dev/null || echo "$OUT"):/o" ubuntu:18.04 bash -ec "
  apt-get update -qq >/dev/null
  apt-get install -y -qq gcc-arm-linux-gnueabihf make bc xz-utils curl ca-certificates libssl-dev flex bison kmod python3 >/dev/null
  cd /src
  if [ ! -d linux-$V ]; then
    curl -fsSLO https://cdn.kernel.org/pub/linux/kernel/v4.x/linux-$V.tar.xz
    curl -fsSL https://cdn.kernel.org/pub/linux/kernel/v4.x/sha256sums.asc | grep \" linux-$V.tar.xz\\$\" | sha256sum -c -
    tar xf linux-$V.tar.xz
  fi
  cd linux-$V
  M='ARCH=arm CROSS_COMPILE=arm-linux-gnueabihf- LOCALVERSION=-sunxi'
  make \$M mrproper >/dev/null
  cp /o/config .config
  make \$M olddefconfig >/dev/null
  grep -q 'CONFIG_ATH9K_HTC=m' .config
  make \$M modules_prepare >/dev/null
  make \$M -j\$(nproc) KBUILD_MODPOST_WARN=1 M=net/rfkill modules
  make \$M -j\$(nproc) KBUILD_MODPOST_WARN=1 M=net/wireless modules
  make \$M -j\$(nproc) KBUILD_MODPOST_WARN=1 M=net/mac80211 modules
  make \$M -j\$(nproc) KBUILD_MODPOST_WARN=1 M=drivers/net/wireless/ath modules
  # CCMP/GCMP: o mac80211 aloca ccm(aes) em ieee80211_key_alloc (aead_api.c:94); o .config tem CRYPTO_CCM=m e o rootfs Alpine
  # não tem /lib/modules do Armbian, então sem ccm.ko o set_key da PTK volta -ENOENT.
  make \$M -j\$(nproc) KBUILD_MODPOST_WARN=1 M=crypto modules
  K=/o/out/lib/modules/$V-sunxi; rm -rf /o/out/lib; mkdir -p \$K
  for f in net/rfkill/rfkill net/wireless/cfg80211 net/mac80211/mac80211 drivers/net/wireless/ath/ath drivers/net/wireless/ath/ath9k/ath9k_hw drivers/net/wireless/ath/ath9k/ath9k_common drivers/net/wireless/ath/ath9k/ath9k_htc crypto/ccm crypto/gcm crypto/ghash-generic; do
    arm-linux-gnueabihf-strip --strip-debug \$f.ko; cp \$f.ko \$K/
  done
  cd \$K; for f in *.ko; do echo \"\$f: \$(modinfo -F vermagic \$f)\"; done | tee /o/vermagic.txt
  modinfo -F depends ath9k_htc.ko; modinfo -F alias ath9k_htc.ko | grep -i 0cf3p7015
"
# firmware do pacote Alpine linux-firmware-ath9k_htc (mesma versão do rootfs)
MSYS_NO_PATHCONV=1 docker run --rm -v "$(cd "$OUT" && pwd -W 2>/dev/null || echo "$OUT"):/o" alpine:3.20 sh -ec "
  mkdir -p /o/out/lib/firmware /t && cd /t && apk update -q && apk fetch -q linux-firmware-ath9k_htc && tar xzf linux-firmware-ath9k_htc-*.apk 2>/dev/null || true
  find /t -name '*htc_7010*'; F=\$(find /t -name 'htc_7010*.fw' | head -1); cp \"\$F\" /o/out/lib/firmware/htc_7010.fw"
du -ch "$OUT"/out/lib/modules/*/*.ko "$OUT"/out/lib/firmware/htc_7010.fw | tail -1
