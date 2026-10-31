# Script de boot A/B do AI Cluster OS (F15 J5). Roda no u-boot a partir da partição FAT `boot`.
#
# Estado em texto no setor 2016 do cartão (fora de qualquer partição; o Linux o grava com dd):
#   aic_slot   = A|B   slot confirmado
#   aic_try    = A|B   slot em teste (vazio se não há teste)
#   aic_tries  = ""|x|xx   tentativas já feitas do slot em teste
#   aic_boots  = x...      boots desde a última confirmação; no sexto, vai direto à recuperação
# O sistema confirma o slot em teste só depois que o worker se registra (aic-slot-commit); sem isso,
# a segunda tentativa que falha volta ao slot confirmado. Contador em letras: sem aritmética.
# Falhando tudo, cai no ambiente de recuperação pela rede (nb_server/nb_args, que o instalador põe no topo deste script) e, por último, no u-boot.

echo "== AI.Cluster - AI Cluster OS =="
setenv load_addr 0x44000000
setenv aic_slot A
setenv aic_try ""
setenv aic_tries ""
setenv aic_boots ""
setenv consoleargs "console=ttyS0,115200 console=tty1"

# O estado mora num setor cru do cartão (AIC_STATE_LBA), nunca num arquivo da FAT: o fatwrite do
# u-boot de 2018 corrompe arquivos vizinhos. Setor em branco = estado padrão acima.
setenv aic_state_lba 0x7e0
if mmc read ${load_addr} ${aic_state_lba} 1; then
	env import -t ${load_addr} 0x200 aic_slot aic_try aic_tries aic_boots
fi

setenv aic_boot ${aic_slot}
if test "${aic_boots}" != "xxxxxx"; then setenv aic_boots "${aic_boots}x"; fi
if test -n "${aic_try}"; then
	if test "${aic_tries}" = "xx"; then
		echo "aic: o slot ${aic_try} falhou duas vezes; voltando ao ${aic_slot}"
		setenv aic_try ""
		setenv aic_tries ""
	else
		setenv aic_tries "${aic_tries}x"
		setenv aic_boot ${aic_try}
	fi
fi
mw.b ${load_addr} 0 0x200
env export -t -s 0x200 ${load_addr} aic_slot aic_try aic_tries aic_boots
mmc write ${load_addr} ${aic_state_lba} 1

if test "${aic_boot}" = "B"; then setenv aic_part 3; else setenv aic_part 2; fi
echo "aic: iniciando o slot ${aic_boot} (mmcblk0p${aic_part}), tentativas=${aic_tries}"

# Um arquivo `recovery` na FAT força a recuperação pela rede (instalador e resgate).
if fatload mmc 0:1 ${load_addr} recovery; then setenv aic_boots xxxxxx; fi

if test "${aic_boots}" != "xxxxxx"; then
	if fatload mmc 0:1 ${kernel_addr_r} zImage; then
		if fatload mmc 0:1 ${fdt_addr_r} dtb; then
			setenv bootargs "root=/dev/mmcblk0p${aic_part} ro rootwait rootfstype=ext4 ${consoleargs} panic=10 loglevel=4 aic.slot=${aic_boot}"
			bootz ${kernel_addr_r} - ${fdt_addr_r}
		fi
	fi
else
	echo "aic: seis boots sem confirmacao; indo para a recuperacao"
fi

echo "aic: os slots nao iniciaram; tentando a recuperacao pela rede"
# nb_server e nb_args vêm de linhas `setenv` que o instalador (install-disk.py) coloca antes deste script.
if test -n "${nb_server}"; then
	echo "aic: servidor de recuperacao=${nb_server}"
	setenv autoload no
	if dhcp; then
		setenv serverip ${nb_server}
		echo "aic: serverip=${serverip}"
		if tftpboot ${kernel_addr_r} ${nb_server}:netboot/zImage; then
			if tftpboot ${ramdisk_addr_r} ${nb_server}:netboot/initramfs.cpio.gz; then
				setenv nb_initrd_size ${filesize}
				if tftpboot ${fdt_addr_r} ${nb_server}:netboot/dtb; then
					setenv bootargs "console=ttyS0,115200 console=tty1 ip=dhcp panic=10 loglevel=4 ${nb_args}"
					bootz ${kernel_addr_r} ${ramdisk_addr_r}:${nb_initrd_size} ${fdt_addr_r}
				fi
			fi
		fi
	fi
fi
# Sem recuperacao (servidor ausente, fora do ar ou com o limite esgotado): nao prender o worker num ciclo
# de reset. Zera o contador e tenta o slot confirmado.
echo "aic: sem recuperacao disponivel; tentando o slot ${aic_slot}"
setenv aic_boots ""
setenv aic_try ""
setenv aic_tries ""
mw.b ${load_addr} 0 0x200
env export -t -s 0x200 ${load_addr} aic_slot aic_try aic_tries aic_boots
mmc write ${load_addr} ${aic_state_lba} 1
if test "${aic_slot}" = "B"; then setenv aic_part 3; else setenv aic_part 2; fi
if fatload mmc 0:1 ${kernel_addr_r} zImage; then
	if fatload mmc 0:1 ${fdt_addr_r} dtb; then
		setenv bootargs "root=/dev/mmcblk0p${aic_part} ro rootwait rootfstype=ext4 ${consoleargs} panic=10 loglevel=4 aic.slot=${aic_slot}"
		bootz ${kernel_addr_r} - ${fdt_addr_r}
	fi
fi
echo "aic: nada iniciou; reiniciando em 10 s"
sleep 10
reset
