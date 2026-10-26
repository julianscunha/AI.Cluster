---
title: "F00 — Validação em hardware real"
aliases: ["Orange Pi", "cluster x86 + ARM", "hardware real"]
tags: [implementacao/fase/00, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-00-fundacao]]"
sistema: ["[[capability-engine]]"]
evidencia: ["code/tests/phase-r-lan.sh"]
---
# F00 — Validação em hardware real

- [ ] Superado

## Objetivo

Tirar da mesma máquina o que só tinha sido provado com containers: um cluster com nós físicos de arquiteturas diferentes numa LAN real.

## Bancada

- Controller, worker `pc-x86` e gateway no PC (x86_64, containers do Docker Desktop).
- Worker num **Orange Pi PC** (Allwinner H3, **armv7l 32 bits**, 4 núcleos, 1 GB de RAM, Ubuntu 18.04, kernel 4.19, glibc 2.27, cartão SD), em outro equipamento da mesma LAN.
- O Pi não compila: o worker e a CLI são cross-compilados no PC por `code/scripts/build-armv7.sh` (zig como linker, glibc 2.27 fixa) e copiados.
- `phase-r-lan.sh` confere um cluster já montado; não sobe nada.

## O que foi validado

| Item | Resultado |
|---|---|
| F00, critério de saída: duas máquinas trocam unidades e comparam compute × transferência | 12 unidades, 9 no x86 (0,6 a 2,5 ms) e 3 no ARM (7,5 a 34 ms); transferência de 24 a 116 ms pela LAN |
| Paridade numérica x86 × ARM | a mesma unidade dá checksum idêntico nas duas arquiteturas (`phase-r-lan.sh`, C) |
| Scores no cluster heterogêneo | COMPUTE 1,05 (x86) contra 0,02 (ARM), coerente com o tempo medido; o placement deu menos trabalho ao ARM |
| Falha real: worker do Pi morto (SIGKILL) com unidade em voo | `OFFLINE` após 15 s de graça, 1 unidade reenfileirada, lote de 6 concluído no x86, 0 falhas; o Pi volta a `READY` sozinho depois |
| Carga térmica | a 100% de CPU o Pi foi de 42 °C a 69 °C (leitura real de `/sys/class/thermal`) e voltou a 53 °C; a saúde ficou em 0,89 (abaixo do limiar térmico) |
| Inferência fatiada com o Pi como shard | SmolLM2 com fatias no x86 e no ARM gerou ` the capital of the country`, igual ao modelo local; 600 chamadas, 41 ms cada, 0 refeitas |
| F15 J3: `bootstrap.sh` num Linux real com systemd 237 | serviço `enabled`, usuário `ai-cluster`, `worker.env` 0640, nó `orangepi-boot` `READY` |

## O que a bancada achou (e foi corrigido)

- **O cartão SD saía como `ssd`.** SD/eMMC (`mmcblk*`) também dizem `rotational=0`. Agora têm a classe `mmc`, entre SSD e HDD.
- **`zram*` contava como armazenamento.** É swap comprimida em RAM. Agora é ignorada.
- **A sonda de disco media a RAM.** Neste Pi o `/tmp` é tmpfs: o cartão que escreve a 5,7 MB/s saiu com score 1,56. A sonda agora foge de tmpfs/ramfs (`/var/tmp`) e aceita `AI_CLUSTER_STORAGE_PROBE_DIR`; o score caiu para 0,01.

## Limites

- O ARM testado é **32 bits (armv7)**: o item "CPU ARM64" da F16 segue aberto.
- O nó x86 é um container num Docker Desktop (VM), não um Linux nu; a LAN e o ARM são físicos.
- Nenhuma imagem de disco, reimage ou A/B (F15 J4, J5) foi testada: exigiria apagar o cartão do Pi.
- O limiar térmico em que a saúde cai não foi alcançado (69 °C); falta aquecer mais, com dissipador removido ou carga mais longa.
- Rede em texto claro (`AI_CLUSTER_INSECURE=1`) com tokens de teste, por ser uma LAN fechada de bancada.
