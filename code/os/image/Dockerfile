# Imagem base imutável do AI Cluster OS (F15 J1): Alpine armv7 com só o necessário para ser worker.
# Construída com `docker build --platform linux/arm/v7` (o Docker Desktop emula o armv7) e exportada
# para um rootfs; veja build-rootfs.sh.
FROM alpine:3.20

# openrc: init e serviços; dropbear: SSH só por chave; chrony: hora (placas sem RTC nascem em 1970
# e certificados TLS não validam com a hora errada).
RUN apk add --no-cache openrc busybox-openrc dropbear chrony tzdata busybox-mdev-openrc haveged openssl \
 && addgroup -S ai-cluster && adduser -S -D -H -h /data/state -s /sbin/nologin -G ai-cluster ai-cluster \
 && rm -rf /var/cache/apk/*

COPY overlay/ /
COPY ai-cluster-worker /usr/bin/ai-cluster-worker
COPY ggml-rpc-server /usr/bin/ggml-rpc-server
COPY aic-release.d/ /etc/aic-release.d/
# versão da imagem: o worker a reporta ao controller (rollout, F19)
ARG AIC_RELEASE=dev
RUN echo "$AIC_RELEASE" > /etc/aic-release

# O rootfs sai de um contêiner, mas roda em hardware: sem isto o OpenRC se acha num contêiner e pula
# localmount, networking e outros (keyword -docker).
RUN rm -f /.dockerenv && echo 'rc_sys=""' >> /etc/rc.conf && echo 'WATCHDOG_DEV=/dev/watchdog' >> /etc/conf.d/watchdog && echo 'WATCHDOG_OPTS="-t 5 -T 15"' >> /etc/conf.d/watchdog && mkdir -p /data /mnt/boot && rm -rf /var/lock && ln -s /run/lock /var/lock && chmod 755 /usr/bin/ai-cluster-worker /usr/bin/ggml-rpc-server /etc/init.d/aic-dns /etc/init.d/ai-cluster-worker /etc/init.d/aic-slot-commit /etc/init.d/aic-dirs /etc/init.d/aic-updater /usr/sbin/aic-updater /usr/sbin/aic-update-request /usr/sbin/aic /usr/sbin/aic-console /usr/sbin/aic-role /usr/sbin/aic-change /usr/sbin/aic-welcome /usr/sbin/aic-passwd /usr/sbin/aic-splash-run /etc/init.d/aic-splash /etc/init.d/aic-measure \
 # raiz somente leitura: o que muda vive em /data (disco) ou em tmpfs
 && ln -sf /data/ssh /root/.ssh && rm -rf /etc/dropbear && ln -s /data/ssh/dropbear /etc/dropbear && rc-update add aic-dirs boot \
 && echo aicluster > /etc/hostname \
  && echo 'makestep 1 -1' >> /etc/chrony/chrony.conf \
 && sed -i 's|^tty1::.*|tty1::respawn:/usr/sbin/aic-console|; /^tty[2-6]::/d' /etc/inittab \
 && rc-update add devfs sysinit && rc-update add aic-splash sysinit && rc-update add mdev sysinit && rc-update add haveged boot && rc-update add dmesg sysinit && rc-update add procfs boot && rc-update add sysfs sysinit \
 && rc-update add swclock boot && rc-update add localmount boot && rc-update add modules boot && rc-update add sysctl boot \
 && rc-update add hostname boot && rc-update add bootmisc boot && rc-update add syslog boot \
 && rc-update add networking boot && rc-update add aic-dns default && rc-update add chronyd default \
 && rc-update add dropbear default && rc-update add ai-cluster-worker default && rc-update add aic-slot-commit default && rc-update add aic-updater default && rc-update add aic-measure default && rc-update add watchdog boot \
 && rc-update add killprocs shutdown && rc-update add mount-ro shutdown && rc-update add savecache shutdown
