# Imagem base imutável do AI Cluster OS para x86_64 UEFI: Alpine com kernel linux-lts, initramfs e
# só o necessário para ser worker. Contexto de build: code/os/image (o overlay comum e o do x86).
FROM alpine:3.20

# linux-firmware-none antes: satisfaz a dependência do kernel sem puxar centenas de MB de firmware
RUN apk add --no-cache linux-firmware-none && apk add --no-cache openrc busybox-openrc dropbear chrony tzdata busybox-mdev-openrc haveged openssl \
      linux-lts mkinitfs e2fsprogs \
 && addgroup -S ai-cluster && adduser -S -D -H -h /data/state -s /sbin/nologin -G ai-cluster ai-cluster \
 && rm -rf /var/cache/apk/*

COPY overlay/ /
COPY x86/overlay/ /
COPY ai-cluster-worker /usr/bin/ai-cluster-worker
COPY aic-release.d/ /etc/aic-release.d/
ARG AIC_RELEASE=dev
RUN echo "$AIC_RELEASE" > /etc/aic-release

# initramfs com os drivers de disco (ext4 e os controladores são módulos no linux-lts) e blkid, para o
# init resolver root=PARTUUID= (o init do Alpine só entende UUID= e LABEL=)
COPY x86/init-pre.sh x86/init-post.sh /tmp/
RUN apk add --no-cache blkid \
 && echo '/sbin/blkid' > /etc/mkinitfs/features.d/aic.files \
 && echo 'features="base ext4 scsi ata nvme virtio usb-storage aic"' > /etc/mkinitfs/mkinitfs.conf \
 && sed -i -e '/^# check if root=... was set$/r /tmp/init-pre.sh' \
           -e 's|^\t\t\$KOPT_root$|\t\t$KOPT_root_wait|' \
           -e '/^\t\t\$KOPT_root_wait$/r /tmp/init-post.sh' \
    /usr/share/mkinitfs/initramfs-init \
 && grep -c 'KOPT_root_wait\|AIC_PARTUUID' /usr/share/mkinitfs/initramfs-init \
 && mkinitfs -o /boot/initramfs-lts "$(ls /lib/modules | head -n 1)" \
 && rm -f /tmp/init-pre.sh /tmp/init-post.sh

RUN rm -f /.dockerenv && echo 'rc_sys=""' >> /etc/rc.conf && mkdir -p /data /mnt/boot && rm -rf /var/lock && ln -s /run/lock /var/lock \
 && chmod 755 /usr/bin/ai-cluster-worker /etc/init.d/ai-cluster-worker /etc/init.d/aic-slot-commit /etc/init.d/aic-dirs /etc/init.d/aic-updater /usr/sbin/aic-updater /usr/sbin/aic-update-request /usr/sbin/aic-console /usr/sbin/aic-role /etc/init.d/aic-measure \
 && ln -sf /data/ssh /root/.ssh && rm -rf /etc/dropbear && ln -s /data/ssh/dropbear /etc/dropbear && rc-update add aic-dirs boot \
 && echo aicluster > /etc/hostname \
 && sed -i 's|^tty1::.*|tty1::respawn:/usr/sbin/aic-console|' /etc/inittab \
 && sed -i 's|^#ttyS0.*|ttyS0::respawn:/sbin/getty -L 115200 ttyS0 vt100|' /etc/inittab \
 && rc-update add devfs sysinit && rc-update add mdev sysinit && rc-update add hwdrivers sysinit && rc-update add haveged boot && rc-update add dmesg sysinit && rc-update add procfs boot && rc-update add sysfs sysinit \
 && rc-update add hwclock boot && rc-update add localmount boot && rc-update add modules boot && rc-update add sysctl boot \
 && rc-update add hostname boot && rc-update add bootmisc boot && rc-update add syslog boot \
 && rc-update add networking boot && rc-update add chronyd default \
 && rc-update add dropbear default && rc-update add ai-cluster-worker default && rc-update add aic-slot-commit default && rc-update add aic-updater default && rc-update add aic-measure default \
 && rc-update add killprocs shutdown && rc-update add mount-ro shutdown && rc-update add savecache shutdown
