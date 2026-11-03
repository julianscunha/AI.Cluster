# Imagem base imutável do AI Cluster OS (F15 J1): Alpine armv7 com só o necessário para ser worker.
# Construída com `docker build --platform linux/arm/v7` (o Docker Desktop emula o armv7) e exportada
# para um rootfs; veja build-rootfs.sh.
FROM alpine:3.20

# wireless-regdb vem do Alpine 3.19: o p7s da 3.20 e assinado por wens, mas o cfg80211 do kernel 4.19.20 do Pi so embute a chave sforshee.
# openrc: init e serviços; dropbear: SSH só por chave; chrony: hora (placas sem RTC nascem em 1970
# e certificados TLS não validam com a hora errada).
RUN apk add --no-cache openrc busybox-openrc dropbear chrony tzdata busybox-mdev-openrc haveged openssl wpa_supplicant iw \
 && apk add --no-cache --repositories-file /dev/null -X https://dl-cdn.alpinelinux.org/alpine/v3.19/main wireless-regdb \
 && addgroup -S ai-cluster && adduser -S -D -H -h /data/state -s /sbin/nologin -G ai-cluster ai-cluster \
 && apk add --no-cache font-terminus && mkdir -p /usr/share/aic/fonts \
 && for n in 16 20 24 32; do cp /usr/share/consolefonts/ter-v${n}n.psf.gz /usr/share/aic/fonts/; done \
 && apk del font-terminus \
 && rm -rf /var/cache/apk/*

COPY overlay/ /
COPY ai-cluster-worker /usr/bin/ai-cluster-worker
COPY aic /usr/bin/aic
COPY ggml-rpc-server /usr/bin/ggml-rpc-server
COPY aic-release.d/ /etc/aic-release.d/
# módulos e firmware do Wi-Fi do Pi (build-kernel-wifi.sh); a pasta vem vazia quando não foram gerados
COPY wifi-lib/ /lib/
RUN if [ -d /lib/modules/4.19.20-sunxi ]; then touch /lib/modules/4.19.20-sunxi/modules.builtin /lib/modules/4.19.20-sunxi/modules.order && depmod -b / 4.19.20-sunxi && cd /lib/modules/4.19.20-sunxi && printf '%s\n' 'rfkill.ko:' 'ghash-generic.ko:' 'gcm.ko: ghash-generic.ko' 'ccm.ko:' 'cfg80211.ko: rfkill.ko' 'mac80211.ko: cfg80211.ko rfkill.ko' 'ath.ko: cfg80211.ko rfkill.ko' 'ath9k_hw.ko: ath.ko mac80211.ko cfg80211.ko rfkill.ko' 'ath9k_common.ko: ath9k_hw.ko ath.ko mac80211.ko cfg80211.ko rfkill.ko' 'ath9k_htc.ko: ath9k_common.ko ath9k_hw.ko ath.ko mac80211.ko cfg80211.ko rfkill.ko' > modules.dep && rm -f modules.dep.bin && printf '%s
' ghash-generic gcm ccm ath9k_htc >> /etc/modules; fi
# o wlan0 pega DHCP com o udhcpc do netwatch: o resolv.conf fica em /run (a raiz e somente leitura)
RUN mkdir -p /etc/udhcpc && echo 'RESOLV_CONF="/run/resolv.conf"' >> /etc/udhcpc/udhcpc.conf
# versão da imagem: o worker a reporta ao controller (rollout, F19)
ARG AIC_RELEASE=dev
RUN echo "$AIC_RELEASE" > /etc/aic-release

# O rootfs sai de um contêiner, mas roda em hardware: sem isto o OpenRC se acha num contêiner e pula
# localmount, networking e outros (keyword -docker).
RUN rm -f /.dockerenv && echo 'rc_sys=""' >> /etc/rc.conf && echo 'WATCHDOG_DEV=/dev/watchdog' >> /etc/conf.d/watchdog && echo 'WATCHDOG_OPTS="-t 5 -T 15"' >> /etc/conf.d/watchdog && mkdir -p /data /mnt/boot && rm -rf /var/lock && ln -s /run/lock /var/lock && chmod 755 /usr/bin/ai-cluster-worker /usr/bin/ggml-rpc-server /etc/init.d/aic-dns /etc/init.d/ai-cluster-worker /etc/init.d/aic-slot-commit /etc/init.d/aic-dirs /etc/init.d/aic-updater /usr/sbin/aic-updater /usr/sbin/aic-update-request /usr/bin/aic /usr/libexec/aic/setup /usr/libexec/aic/role /usr/libexec/aic/change /usr/libexec/aic/passwd /usr/libexec/aic/wifi /usr/libexec/aic/netwatch /etc/init.d/aic-wifi /usr/sbin/aic-console /usr/sbin/aic-welcome /usr/sbin/aic-splash-run /etc/init.d/aic-splash /etc/init.d/aic-measure \
 # raiz somente leitura: o que muda vive em /data (disco) ou em tmpfs
 && ln -sf /data/ssh /root/.ssh && rm -rf /etc/dropbear && ln -s /data/ssh/dropbear /etc/dropbear && rc-update add aic-dirs boot \
 && echo aicluster > /etc/hostname \
  && echo 'makestep 1 -1' >> /etc/chrony/chrony.conf \
 && sed -i 's|^tty1::.*|tty1::respawn:/usr/sbin/aic-console|; /^tty[2-6]::/d' /etc/inittab \
 && rc-update add devfs sysinit && rc-update add aic-splash sysinit && rc-update add mdev sysinit && rc-update add haveged boot && rc-update add dmesg sysinit && rc-update add procfs boot && rc-update add sysfs sysinit \
 && rc-update add swclock boot && rc-update add localmount boot && rc-update add modules boot && rc-update add sysctl boot \
 && rc-update add hostname boot && rc-update add bootmisc boot && rc-update add syslog boot \
 && rc-update add networking boot && rc-update add aic-dns default && rc-update add aic-wifi default && rc-update add chronyd default \
 && rc-update add dropbear default && rc-update add ai-cluster-worker default && rc-update add aic-slot-commit default && rc-update add aic-updater default && rc-update add aic-measure default && rc-update add watchdog boot \
 && rc-update add killprocs shutdown && rc-update add mount-ro shutdown && rc-update add savecache shutdown
