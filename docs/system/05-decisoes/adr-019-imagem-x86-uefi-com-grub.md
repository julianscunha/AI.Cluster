---
title: "ADR-019 — Imagem x86_64 UEFI com GRUB e A/B por partição"
aliases: ["ADR-019", "imagem x86", "AI Cluster OS x86_64"]
tags: [sistema/adr, subsistema/ai-cluster-os]
type: sistema/adr
---

# ADR-019 — Imagem x86_64 UEFI com GRUB e A/B por partição

Status: decidido · Data: 2026-10-27 · Voltar para [[decisoes]].

## Contexto

O AI Cluster OS ([[adr-002-os-imutavel]], [[adr-017-instalacao-a-b-e-boot-de-rede]]) existia só para placas armv7 com u-boot. PCs, notebooks e servidores x86_64 são a maior parte do hardware reaproveitável e precisam do mesmo sistema: imutável, com slots A/B, rollback automático e release assinada ([[adr-018-release-assinada-e-medida-do-slot]]).

## Decisão

1. **Disco GPT com quatro partições:** ESP FAT32 (64 MiB), slot A e slot B (1 GiB cada, ext4) e `aic-data`. O kernel (`linux-lts` do Alpine) e o initramfs ficam **dentro do slot**, em `/boot`: kernel e raiz mudam juntos, em uma gravação só.
2. **GRUB EFI embutido e imutável** (`EFI/BOOT/BOOTX64.EFI`, gerado com `grub-mkstandalone`), com a lógica do `boot.cmd` do armv7. O estado do A/B é o `grubenv` da ESP (`aic_slot`, `aic_try`, `aic_tries`): slot em teste com duas tentativas e volta ao slot confirmado na terceira.
3. **A identidade do slot é o número da partição no disco da raiz**, nunca um rótulo do ext4: a mesma imagem assinada vai para os dois slots, e o hash cobre o ext4 inteiro, então rótulo por slot invalidaria a medida. O GRUB passa `root=PARTUUID=` e o initramfs resolve com `blkid` (o init do Alpine só entende `UUID=` e `LABEL=`). No sistema, os slots são as partições 2 e 3 **do mesmo disco da raiz em execução**, o que evita agir num segundo disco com o mesmo layout.
4. **Uma camada por plataforma** (`/usr/lib/aic/ab.sh`) com `slot_dev`, `state_get` e `state_set`. `aic-updater`, `aic-slot-commit` e `aic-measure` são os mesmos nas duas plataformas. No x86 a ESP é montada só no instante da leitura ou da escrita, com lock, e o `grubenv` é regravado no lugar (bloco de 1024 bytes, como o `save_env`).
5. **Release assinada para a placa `x86_64-uefi`.** A mensagem assinada já inclui a placa, então uma release do armv7 não instala no x86.
6. **Instalação por gravação do disco inteiro** (`install-disk.sh`, a partir de qualquer Linux), que move a GPT de backup para o fim, sorteia os GUIDs e estende a partição de dados.

## Consequências

- O kernel entra na medida do slot, o que não acontece no armv7.
- Mesmo fluxo de rollout, canário e rollback do armv7, validado em QEMU com OVMF (ver abaixo).
- A imagem exige Secure Boot **desligado**: o Alpine não tem shim assinado.
- O firmware de hardware (`linux-firmware`) fica de fora para a imagem caber no slot; placas de rede e vídeo que precisam de firmware exigem um pacote a mais na imagem.
- A imagem cresce por release; a partição de 1 GiB dá folga e o tamanho gravado (`slot_mib`) vem do `/etc/aic-board` de cada imagem.

## Alternativas

- **Rótulo de ext4 por slot (`aic-a`/`aic-b`):** descartada, quebra a medida assinada.
- **systemd-boot com entradas e contador de tentativas:** descartada por exigir kernel unificado e outro conjunto de ferramentas; o `grubenv` reaproveita a lógica já validada no armv7.
- **Estado em NVRAM UEFI:** descartada, varia entre firmwares e não sobrevive a troca de disco.

## Limites

- **Validada em QEMU (TCG), não em hardware real.** Boot UEFI, registro no controller, rollout 1.0 para 2.0 com troca de slot, medida igual à assinada, persistência da confirmação num reboot e rollback de uma release cujo worker não se registra (duas tentativas e volta ao slot anterior). A mesma refatoração foi revalidada no Orange Pi (armv7).
- O instalador foi exercitado até a edição da GPT num dispositivo de loop; o redimensionamento do ext4 de dados não foi exercitado num disco real.
- Sem recuperação por rede e sem PXE no x86; se os dois slots falham, o GRUB cai no slot confirmado.
- Sem watchdog de hardware configurado fora do QEMU (`i6300esb`); em hardware real depende do driver da placa-mãe.
- Sem Secure Boot, dm-verity nem TPM ([[adr-018-release-assinada-e-medida-do-slot]]).
- Uma imagem suporta uma placa só (`/etc/aic-board`).
