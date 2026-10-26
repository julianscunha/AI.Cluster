---
title: "F15 — Imagem A/B, reimage e recuperação por boot de rede"
aliases: ["J4", "J5", "reimage", "boot A/B"]
tags: [implementacao/fase/15, implementacao/fatia, implementacao/estado/parcial]
type: implementacao/fatia
status: parcial
superado: false
fase: "[[fase-15-ai-cluster-os]]"
codigos-antigos: ["J4", "J5"]
sistema: ["[[ai-cluster-os]]", "[[provisionamento]]", "[[adr-017-instalacao-a-b-e-boot-de-rede]]"]
evidencia: ["code/tests/phase-u-netboot.sh", "code/os/image/", "code/os/netboot/"]
---
# F15 — Imagem A/B, reimage e recuperação por boot de rede

- [ ] Superado

## Objetivo

J4: reimage de uma máquina em instalação limpa. J5: boot e recuperação A/B. A decisão está no [[adr-017-instalacao-a-b-e-boot-de-rede]].

## O que foi feito

- **Imagem** (`code/os/image/`): Alpine 3.20 armv7 com OpenRC, worker musl, dropbear (só chave), chrony e haveged (placa sem RNG); raiz ext4 somente leitura, `/var/log` e `/tmp` em tmpfs, `/data` ext4 em partição própria. `build-rootfs.sh` exporta o contêiner e cria o ext4 com `mke2fs -d` (nada montado no PC); `mk-disk-image.sh` monta o disco inteiro (cabeçalho do u-boot + FAT `AICBOOT` + slots A e B de 512 MiB + `aicdata`).
- **`boot.cmd`**: estado `aic_slot/aic_try/aic_tries/aic_boots` no setor cru 2016; slot em teste com duas tentativas; seis boots sem confirmação ou um arquivo `recovery` levam à recuperação por rede (DHCP + TFTP); por último, `reset`.
- **`aic-slot-commit`**: confirma o slot depois que o worker registra (ou o controller está inalcançável); se o controller responde e o worker não entra em 300 s, reinicia. **`aic-dirs`** cria `/data/ssh/dropbear` e `/data/state` antes do dropbear e do worker.
- **`install-disk.py`**: rodando do ambiente em RAM, confere raiz `rootfs`, cartão desmontado e `--yes`, grava a imagem e escreve `worker.env` (0600), a chave SSH e o `boot.scr` com as definições da recuperação. **`ubootscr.py`** gera script do u-boot sem `mkimage`.
- **Boot de rede** (`code/os/netboot/`): `tftpd.py` (somente leitura, `--allow`, `--boots`, tolera perda de pacote), `build-initramfs.sh` (busybox + dropbear + worker; o token vem pela linha de comando), `install-netboot.sh` (boot de rede sobre um Armbian existente, com prazo e volta ao cartão).

## Evidência (Orange Pi PC, armv7, SD de 3,9 GB)

- Gravação de 3,9 GB em ~530 s, boot do cartão em ~20 s e nó `orangepi-ab` READY.
- Atualização: B gravado e marcado em teste → sobe em B, o worker se registra e o estado vira `aic_slot=B`.
- Rollback: B zerado e em teste → duas falhas e volta ao A em ~75 s com o estado limpo.
- Recuperação: A e B zerados → o u-boot cai na rede, baixa kernel/initramfs/dtb e sobe a recuperação em RAM; os slots foram regravados por ela e o nó voltou ao A.
- `code/tests/phase-u-netboot.sh`: ciclo do boot de rede com retorno automático ao cartão (teste de bancada, exige o hardware).

## Achados em hardware (que mudaram o desenho)

- O OpenRC detectou `[DOCKER]` (por `/.dockerenv` no rootfs exportado) e pulou `localmount`, `networking` e outros: `rc_sys=""` e remoção do arquivo.
- `fatwrite` do u-boot corrompeu arquivos da FAT: o estado foi para um setor cru.
- `env import -t` e `source` de um arquivo extra não aplicaram variáveis: as definições vão no `boot.scr`.
- Sem `serverip`, o u-boot envia os ACKs do TFTP pelo roteador e perde pacotes; com o servidor definido a transferência é estável.
- O TFTP do u-boot travou com blocos de 1468 bytes nesta placa: o servidor limita a 512.

## Limites

> [!warning] Ainda não é o gate da F15
> Só armv7 (Orange Pi); ARM64 e x86 não têm imagem nem instalador (J6, F16). Sem assinatura de boot. O servidor TFTP é um script de bancada. O rollout automático pelo controller (release assinada, canário, medida do slot) está em [[f19-rollout-de-release]]. O `boot.scr` leva o join token em claro. Teste em um só nó.
