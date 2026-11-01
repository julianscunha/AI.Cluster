#!/usr/bin/env python3
"""Monta o usb.zip do modo dedicado (ADR-023): o pendrive de instalação que o operador baixa no navegador.

  python3 mkusb.py <pasta dedicado> <saída.zip> <ip do controller> <porta de espera> <porta do convite>

O zip é extraído na raiz de um pendrive FAT32 qualquer (sem rótulo, letra ou ordem de disco). EFI/BOOT/BOOTX64.EFI é o GRUB
(feito no build, com grub-mkstandalone): percorre os discos até achar /aic/grub.cfg e o carrega (menu); ele leva na linha do kernel o
endereço deste controller (a espera de aprovação é a porta de espera; a imagem vem do convite). Stdlib apenas.
"""

import pathlib
import re
import sys
import zipfile

# O que o zip precisa do build (o disco e as somas são conferidos pelo instalador, com a chave pinada).
PRECISA = ["BOOTX64.EFI", "vmlinuz", "initramfs.gz"]


def grub_cfg(ip, espera, convite):
    # menu navegável (setas + Enter): instala sozinho em 5 s; "Reiniciar" para desistir
    return (
        "set timeout=5\n"
        "set default=0\n"
        'menuentry "Instalar AI Cluster OS" {\n'
        "  linux /aic/vmlinuz console=tty0 console=ttyS0,115200 panic=10 "
        f"aic.controller=http://{ip}:{espera} aic.image=http://{ip}:{convite}/imagem\n"
        "  initrd /aic/initramfs.gz\n"
        "}\n"
        'menuentry "Reiniciar o computador" {\n'
        "  reboot\n"
        "}\n"
    )


def installer_conf(ip, espera, convite):
    return (
        "# informativo: o instalador recebe estes endereços na linha do kernel (aic/grub.cfg)\n"
        f"controller=http://{ip}:{espera}\n"
        f"image=http://{ip}:{convite}/imagem\n"
    )


LEIAME = """AI Cluster - pendrive de instalacao (modo dedicado)
====================================================

1. Formate o pendrive em FAT32 (qualquer rotulo serve).
   No Windows: clique com o botao direito na unidade > Formatar > sistema de
   arquivos FAT32. (A janela do Windows so oferece FAT32
   em pendrives de ate 32 GB; use um deles.)
   APAGA tudo que estiver no pendrive.
2. Extraia o conteudo deste usb.zip na RAIZ do pendrive. Devem aparecer as pastas
   EFI e aic.
3. Desligue o computador que vai virar um no do cluster, ligue o pendrive e entre no
   menu de boot (F12, F11, F9, F8 ou Esc, conforme a placa). Escolha o pendrive com
   "UEFI" no nome. O Secure Boot precisa estar DESLIGADO na BIOS/UEFI.
4. A tela mostra a impressao digital do aparelho. Confira-a com a do controller e
   aprove-o: sudo aic cluster install approve <impressao> --disk <disco>
   (o convite precisa estar aberto: sudo aic cluster invite).
5. So depois da sua aprovacao o disco escolhido e apagado, com uma contagem de 10 s.
   Ao terminar, tire o pendrive; o computador reinicia pelo disco.

Endereco deste controller: {ip}   (convite: porta {convite})

---------------------------------------------------------------------

AI Cluster - installation pendrive (dedicated mode)

1. Format the pendrive as FAT32 (any label works).
   On Windows: right-click the drive > Format > file system FAT32.
   (The Windows dialog only offers FAT32 on pendrives up to 32 GB; use one of those.)
   This ERASES everything on the pendrive.
2. Extract the contents of this usb.zip to the ROOT of the pendrive. The folders
   EFI and aic must appear.
3. Turn off the computer that will join the cluster, plug in the pendrive and open
   the boot menu (F12, F11, F9, F8 or Esc, depending on the board). Choose the
   pendrive with "UEFI" in its name. Secure Boot must be OFF in the BIOS/UEFI.
4. The screen shows the device fingerprint. Check it against the controller and
   approve it: sudo aic cluster install approve <fingerprint> --disk <disk>
   (the invitation must be open: sudo aic cluster invite).
5. Only after your approval is the chosen disk erased, after a 10 s countdown.
   When it finishes, remove the pendrive; the computer restarts from the disk.
"""


def monta(pasta, saida, ip, espera, convite):
    if not re.fullmatch(r"[A-Za-z0-9.-]+", ip):
        raise SystemExit(f"endereço do controller inválido: {ip!r}")
    for porta in (espera, convite):
        if not (str(porta).isdigit() and 0 < int(porta) < 65536):
            raise SystemExit(f"porta inválida: {porta!r}")
    pasta = pathlib.Path(pasta)
    for nome in PRECISA:
        if not (pasta / nome).is_file():
            raise SystemExit(f"falta {nome} em {pasta}: rode build-installer.sh e a release com dedicado/")
    tmp = pathlib.Path(str(saida) + ".tmp")
    with zipfile.ZipFile(tmp, "w", zipfile.ZIP_DEFLATED) as z:
        z.write(pasta / "BOOTX64.EFI", "EFI/BOOT/BOOTX64.EFI")
        z.write(pasta / "vmlinuz", "aic/vmlinuz")
        z.write(pasta / "initramfs.gz", "aic/initramfs.gz")
        z.writestr("aic/grub.cfg", grub_cfg(ip, espera, convite))
        z.writestr("aic/installer.conf", installer_conf(ip, espera, convite))
        z.writestr("LEIAME.txt", LEIAME.format(ip=ip, convite=convite).replace("\n", "\r\n"))
    tmp.replace(saida)


if __name__ == "__main__":
    if len(sys.argv) != 6:
        raise SystemExit(__doc__)
    monta(sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4], sys.argv[5])
