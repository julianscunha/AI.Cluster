#!/usr/bin/env python3
"""Grava a imagem do AI Cluster OS (F15 J4) no cartão de um worker que está no ambiente de recuperação
em RAM (boot de rede) e escreve a identidade dele na partição de dados.

    AIC_SSH_PASSWORD=... python3 install-disk.py --host 192.168.1.24 --yes \\
        --image build/disk.img.gz --controller http://192.168.1.25:50051 \\
        --token TOKEN --node-id orangepi-1 --ssh-key ~/.ssh/aic_pi.pub \\
        --recovery-server 192.168.1.25

Trava de segurança: só age num worker cujo cartão está DESMONTADO e que roda do initramfs (raiz `rootfs`),
isto é, nunca no sistema que está em uso. Apaga o cartão inteiro; exige --yes.
Precisa do paramiko (pip install paramiko). Roda no PC; o worker só precisa de busybox.
"""
import argparse
import ipaddress
import os
import shlex
import sys
import time

import paramiko

from ubootscr import uboot_script

DEVICE = '/dev/mmcblk0'


def run(client, command, timeout=120):
    _, out, err = client.exec_command(command, timeout=timeout)
    code = out.channel.recv_exit_status()
    return code, out.read().decode(errors='replace'), err.read().decode(errors='replace')


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--host', required=True)
    ap.add_argument('--user', default='root')
    ap.add_argument('--image', required=True, help='disk.img.gz')
    ap.add_argument('--controller', required=True)
    ap.add_argument('--token', required=True)
    ap.add_argument('--node-id', required=True)
    ap.add_argument('--ssh-key', help='chave pública que poderá entrar por SSH no worker instalado')
    ap.add_argument('--recovery-server', help='IP do servidor TFTP da recuperação pela rede')
    ap.add_argument('--recovery-token', help='token de recuperação (AI_CLUSTER_RECOVERY_TOKEN do controller); vai em claro no boot.scr, por isso não é o token de join')
    ap.add_argument('--insecure', action='store_true', default=True,
                    help='controller em http (rede de bancada); padrão ligado')
    ap.add_argument('--yes', action='store_true', help='confirma que o cartão inteiro será apagado')
    args = ap.parse_args()

    if args.recovery_server:
        ipaddress.ip_address(args.recovery_server)  # sem isto, uma quebra de linha injetaria comandos no boot.scr
        if not args.recovery_token or args.recovery_token == args.token:
            sys.exit('a recuperação exige --recovery-token, diferente do token de join')
    password = os.environ.get('AIC_SSH_PASSWORD')
    if not password:
        sys.exit('defina AIC_SSH_PASSWORD (a senha não vai na linha de comando)')
    for value in (args.controller, args.token, args.node_id, args.recovery_token or 'x'):
        if not value or any(c not in 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._:/@=+-' for c in value):
            sys.exit(f'caractere não permitido em: {value!r}')

    client = paramiko.SSHClient()
    client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    client.connect(args.host, username=args.user, password=password, timeout=20)

    # 1) Travas: ambiente de RAM e cartão desmontado.
    _, mounts, _ = run(client, 'cat /proc/mounts')
    if not mounts.splitlines()[0].startswith('rootfs / rootfs'):
        sys.exit('o worker não está rodando do ambiente em RAM (raiz != rootfs); recuso apagar um sistema em uso')
    if 'mmcblk0' in mounts:
        sys.exit('há partições do cartão montadas; recuso gravar')
    _, size, _ = run(client, f'cat /sys/block/{DEVICE.split("/")[-1]}/size')
    card_bytes = int(size.strip()) * 512
    with open(args.image, 'rb') as f:  # ISIZE: tamanho original nos 4 últimos bytes do gzip
        f.seek(-4, 2)
        image_bytes = int.from_bytes(f.read(4), 'little')
    print(f'cartão: {card_bytes} bytes; imagem: {image_bytes} bytes')
    if card_bytes < image_bytes:
        sys.exit('o cartão é menor que a imagem')
    if not args.yes:
        sys.exit(f'isto apaga {DEVICE} em {args.host}; rode de novo com --yes')

    # 2) Grava a imagem inteira (o gzip viaja comprimido; o worker descomprime).
    print('gravando a imagem...', flush=True)
    channel = client.get_transport().open_session()
    # o status do gunzip vai a um arquivo: num pipe, só o do dd chegaria a quem chamou
    channel.exec_command(f'{{ gunzip -c; echo $? > /tmp/gz.status; }} | dd of={DEVICE} bs=1M conv=fsync 2>&1')
    sent = 0
    started = time.time()
    with open(args.image, 'rb') as f:
        while chunk := f.read(1 << 20):
            channel.sendall(chunk)
            sent += len(chunk)
    channel.shutdown_write()
    out = b''
    while not channel.exit_status_ready():
        if channel.recv_ready():
            out += channel.recv(4096)
        time.sleep(1)
    out += channel.recv(65536)
    code = channel.recv_exit_status()
    print(out.decode(errors='replace').strip().splitlines()[-1:] or '', f'(saida {code}, {time.time() - started:.0f}s)')
    if code != 0 or run(client, 'cat /tmp/gz.status')[1].strip() != '0':
        sys.exit('falha ao gravar a imagem (dd ou gunzip falhou); refaça antes de reiniciar')
    run(client, 'sync; echo 1 > /proc/sys/vm/drop_caches 2>/dev/null; true')
    run(client, f'echo 1 > /sys/block/{DEVICE.split("/")[-1]}/device/rescan 2>/dev/null; true')
    # Relê a tabela de partições para o kernel enxergar o layout novo.
    run(client, f'blockdev --rereadpt {DEVICE} 2>/dev/null || true')
    time.sleep(2)
    code, listing, _ = run(client, f'ls {DEVICE}p1 {DEVICE}p2 {DEVICE}p3 {DEVICE}p4 2>&1')
    if code != 0:
        sys.exit(f'o kernel não enxergou as partições novas ({listing.strip()}); reinicie o worker e rode de novo só a configuração')

    # 3) Identidade do worker na partição de dados e a recuperação na de boot.
    worker_env = (
        f'AI_CLUSTER_CONTROLLER={args.controller}\n'
        f'AI_CLUSTER_JOIN_TOKEN={args.token}\n'
        f'AI_CLUSTER_NODE_ID={args.node_id}\n'
        'AI_CLUSTER_NODE_CREDENTIAL_FILE=/data/state/credential\n'
        'AI_CLUSTER_MODEL_DIR=/data/models\n'
        'AI_CLUSTER_UPDATE_CMD=/usr/sbin/aic-update-request\n'
        + ('AI_CLUSTER_INSECURE=1\n' if args.insecure and args.controller.startswith('http://') else '')
    )
    cmds = ['mkdir -p /mnt/data /mnt/boot', f'mount -t ext4 {DEVICE}p4 /mnt/data',
            'mkdir -p /mnt/data/state /mnt/data/ssh /mnt/data/models',
            f'(umask 077; printf %s {shlex.quote(worker_env)} > /mnt/data/worker.env)', 'chmod 600 /mnt/data/worker.env']
    if args.ssh_key:
        key = open(os.path.expanduser(args.ssh_key), encoding='utf-8').read().strip()
        cmds += [f'printf "%s\\n" {shlex.quote(key)} > /mnt/data/ssh/authorized_keys',
                 'chmod 700 /mnt/data/ssh', 'chmod 600 /mnt/data/ssh/authorized_keys']
    cmds += ['sync', 'umount /mnt/data']
    code, out, err = run(client, ' && '.join(cmds))
    if args.recovery_server and code == 0:
        # As definições da recuperação pela rede vão dentro do boot.scr: o u-boot as lê sem `source` nem `env import`.
        settings = (f'setenv nb_server {args.recovery_server}\n'
                    f'setenv nb_args "aic.controller={args.controller} aic.node={args.node_id}-rec '
                    f'aic.token={args.recovery_token} aic.ttl=0"\n')
        boot_cmd = open(os.path.join(os.path.dirname(os.path.abspath(__file__)), 'boot.cmd'), encoding='utf-8').read()
        chan = client.get_transport().open_session()
        chan.exec_command(f'mount -t vfat {DEVICE}p1 /mnt/boot && cat > /mnt/boot/boot.scr && sync && umount /mnt/boot')
        chan.sendall(uboot_script(settings + boot_cmd))
        chan.shutdown_write()
        code, err = chan.recv_exit_status(), chan.recv_stderr(4096).decode(errors='replace')

    if code != 0:
        sys.exit(f'falha ao escrever a identidade: {err or out}')
    print('identidade escrita. Reinicie o worker para subir do cartão: reboot -f')


if __name__ == '__main__':
    main()
