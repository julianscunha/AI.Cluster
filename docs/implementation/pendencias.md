---
title: "Pendências"
aliases: ["O que falta", "Pendências do projeto"]
tags: [implementacao/pendencias]
type: implementacao/pendencias
---

# Pendências

Só o que falta, em poucas palavras. O que já existe está no [[guia]] e no sistema; o histórico está no git.

Dependência: **código** (resolve-se no repositório), **hardware** (máquina ou componente que o laboratório não tem: celulares, GPU/NPU, x86, ARM64, TPM, placa armv7 extra, duas máquinas físicas, disco real com SMART), **decisão** (escolha de projeto antes de codar) ou **dados** (corpus ou tarefa para medir).

## Versão pública (beta): critério e bloqueadores

Decisão de 04/11/2026: publicar como **beta/preview**, com limites declarados, e mover o resto para o backlog pós-1.0. Todas as seções abaixo desta são backlog pós-1.0, exceto o que este quadro listar. Só a publicação no GitHub depende de autorização do usuário.

| Critério | Estado |
|---|---|
| Instalação em rede cabeada (pendrive x86, imagem do Pi) com aprovação do controller | feito |
| Atualização assinada com rollback, em Pi armv7 e em dois notebooks x86 | feito (0.3.0 a 0.3.4) |
| Segurança básica: túnel mTLS, três papéis, chaves por tenant, imagem assinada | feito (revogação de chaves de release do SO: pós-1.0) |
| Inferência dividida estável (GGUF no llama.cpp por RPC) | feito (1,5B, 3B e 7B) |
| Desempenho medido e repetível (`aic-bench.sh`) | feito: Qwen 7B em 2 notebooks, 2,09 tok/s |
| Updater repete o download que falhou | **bloqueador** |
| Medição em Wi-Fi e com nó lento na versão final | **bloqueador** (sem isso não há número honesto para o pior cenário) |
| Aviso de limites no README e no guia (velocidade, hardware testado, pendrive só em cabo, mais workers = memória, não velocidade) | **bloqueador** (documentação) |
| Gateway sem loop quando o auxiliar é recusado | recomendado (a especulação é opt-in) |
| Pi: sensor térmico e NEON | recomendado |

**Backlog pós-1.0 (não bloqueia):** paralelismo de tensor (única via que acelera uma requisição com mais workers; precisa de ADR), instalação por pendrive via Wi-Fi, ARM64, GPU/NPU, celulares, Secure Boot e PXE, revogação e CRL, request manager completo, graph engine e edge optimizer, tudo de F11 em diante.

## Ordem de execução (otimizada para não refazer)

Os lotes só referenciam as seções abaixo. Regra: decisão antes de código; medição antes de otimização; tudo que pede imagem nova entra num único rebuild/rollout; hardware por último, em lotes que dividem o mesmo setup físico. Cada lote diz: imagem/release (I) e hardware (H).

**Lote 1 — Decisões e ADRs (sem código; I: não; H: não). ADRs 030 e 031 aprovados pelo usuário em 01/11/2026 (status decidido); implementados `--trust-listed` com habilitação, segredo por gateway, impressão da chave no convite, pino do instalador na chave de Linux, lista de revogação e a regra de roll-forward; falta o downgrade com bandeira assinada (Lote 4).** ADR de Wi-Fi no AI Cluster OS ([[#F17 e F18 — Adaptativo e móvel]]); base Linux, ADR da F03, `--trust-listed`, gateway atrás de NAT, origem do pino, chave que assina o `SHA256SUMS` do instalador, revogação da chave de Linux, downgrade do rollout (seções F15/F16, F07, aic, F19). Por quê: o ADR de Wi-Fi define credencial, firmware e papel do `setup`; sem ele o Lote 4 seria refeito. As demais decisões fixam formato de convite, pino e CRL antes de tocar nesses fluxos.

**Lote 2 — Medições em código/lab, sem imagem (I: não; H: só os 2 PCs do lab).** Ideia 1 do distributed-llama (cálculo x sincronização por token, com a carga lenta do túnel ~4,5 MiB/s e o QUICKACK, ideia 7); custo do atestado e das reservas (só se houver perfil). Por quê: o resultado decide as ideias 3 e 8, o desenho do cache/túnel para Wi-Fi e o marco M4; otimizar antes seria chute.

**Lote 2 — causas prováveis da carga lenta (~4,5 MiB/s, diagnóstico por leitura em `.tmp/tunel-diagnostico.md`):** um SET_TENSOR com ACK por tensor no RPC do ggml; `copy_bidirectional` com buffer de 8 KiB (`crates/tunnel/src/lib.rs:357,414`, trocar por `copy_bidirectional_with_sizes`); sem QUICKACK nem buffers de socket; suíte TLS no armv7; cache `-c` só ajuda a 2ª carga e o `prune_cache` pode expulsar o modelo. Medir com `ss -ti`, `iperf3` direto x pelo túnel e CPU do túnel.

**Lote 3 — Código de servidor/CLI que não entra em imagem (I: só release de binários; H: não).** ideias 4 e 6 (gateway como worker, `aic model pull`), ideia 5 se houver x86 com GPU, (painel Inferência e cache automático já feitos, em main; só validar com llama-server real), inglês na CLI (`AIC_LANG`), níveis de confiança, revogação/mTLS/auditoria, HTTPS no download, gatilho de atualização, F11 em diante só se o produto pedir (Request Manager antes de F12/F13/F17). Por quê: ideias 2 e 4 mudam o plano que o Lote 4 usa; o seletor de idioma altera todas as mensagens, então vem antes de textos novos de console/instalador.

**Lote 4 — Código que muda a imagem do SO (I: sim, o único rebuild; H: não ainda).** Wi-Fi (`wpa_supplicant`/`iw`, firmware por chipset, credencial em `/data`, `networking` com queda para o cabo, plano que desconta Wi-Fi); **Wi-Fi: código do overlay/imagens pronto e testado por script (wpa_supplicant/iw, `aic worker wifi`, `netwatch`, `link: wifi` e desconto de 0,5 no plano); o driver `ath9k_htc` já está no `.config` do kernel do Pi: módulos compilados em `.tmp/kernel-wifi` (`build-kernel-wifi.sh`), falta pô-los no rootfs e testar no Pi; falta a coleta no convite/instalador.** IP por DHCP (reserva/nome no controller, aviso sobre a chave de host do SSH); sonda de disco do Pi (conserto já no código, falta a imagem); sensor térmico do Pi (causa achada: o `.config` tem `THERMAL=m` e `SUN8I_THS=m` e o dtb já traz `cpu_thermal`; sem `/lib/modules` nenhum carrega, então não há `thermal_zone0`; o `SUN8I_THS` é patch do Armbian, ausente do 4.19.20 vanilla, então os módulos `thermal_sys`/`sun8i_ths` exigem a fonte do Armbian em `build-kernel-wifi.sh`; leitura do worker correta e testada); fonte do console maior e textos F2; impressão da credencial, `serving_model` e data/hora no console; critério do slot do controller (serviço respondeu na porta); modo dedicado; TLS/CA fixada no instalador; limpar `rootfs-*.ext4` antigos; ideia 3 (patch do RPC) só se o Lote 2 mostrar a rede como gargalo. Por quê: um único build x86 e um ARMv7 recebem tudo, e cada rollout custa uma sessão de hardware e um risco de queda (ver Lote 5).

**Lote 5 — Diagnóstico do notebook (I: não; H: sim, uma pessoa na tela).** Queda após rollout 0.2.4 e "Hard drive not installed" após 0.2.3 (seção Hardware real). Por quê: antes do rollout do Lote 6 para não perder o aparelho no meio; a causa (BIOS/AHCI, NVMe, `aic-measure` lento) pode exigir ajuste que cairia na imagem do Lote 4, então se achar algo, volte ao Lote 4 antes de seguir.

**Lote 6 — Release única e rollout assinado (I: sim, publica o Lote 4; H: Pi + notebook).** Rollout canário pela atualização assinada (pendrive só para recuperação); valida boot completo da imagem x86 (em máquina com KVM ou hardware real), gatilho com systemd/OpenRC reais, renovação do certificado ponta a ponta. Por quê: serve de teste de regressão de tudo do Lote 4 com um só rollout.

**Lote 7 — Hardware no Pi e no notebook já instalados (I: não; H: mesma bancada).** Rede nos dois sentidos e sob carga, sensor térmico, Pi no limite de RAM/térmico, perda de energia no meio de A/B, console no monitor com teclado real, cluster de 3 a 4 máquinas e runbook, `--spread` com o 7B (`model check`). Por quê: exercitam a imagem do Lote 6 sem trocar o setup; as medições alimentam as ideias 3 e 8.

**Lote 8 — Hardware de rede/boot x86 (I: não; H: PC com UEFI, cabo, roteador).** Secure Boot, PXE e kexec do `/wait`, pendrive em PC real, netboot, firmware opcional (i915, ath10k, regdb), watchdog e resize do ext4 em disco real, SMART/NVMe. Por quê: depende do Lote 6 e fica separado porque pede máquina e BIOS que o Lote 7 não usa; Secure Boot só depois de fechar o formato de boot da imagem.

**Lote 9 — Hardware Wi-Fi (I: só correção se falhar; H: adaptador USB + AP).** Adaptador USB no Orange Pi, reconexão após queda do AP, vazão e atraso do Pi em Wi-Fi; depois decidir cache de tensores/túnel para enlace instável com os números. Por quê: usa o Lote 2 (base de medida) e o Lote 4; fica depois do cabo para comparar com o Lote 7. Depois do teste de Wi-Fi no sistema instalado: instalador por pendrive só com Wi-Fi (linha "Instalação por pendrive via Wi-Fi" em F17 e F18); só tem valor para celular ou notebook novo sem cabo, e muda o initramfs, então entra num rebuild próprio.

**Lote 10 — Hardware novo e fases futuras (I: sim, novas imagens; H: GPU/NPU, ARM64, celulares).** Aceleradores e scheduler por GPU/NPU, release ARM64, worker móvel (Q1 a Q6), ideia 8 (tensor por camada em gigabit), F12 a F18 (estado por chave, execução fora do gateway, P1 a P6). Por quê: dependem de dados dos lotes 2, 7 e 9 e de aparelhos que o laboratório não tem; as imagens ARM64/celular herdam Wi-Fi, console e sonda já fechados.

## F00 a F03 — Fundação, núcleo, capability, execução

| Item | O que falta | Dependência |
|---|---|---|
| Cluster de 3 a 4 máquinas e runbook | Duas máquinas físicas já dividem modelo (Qwen2.5 1.5B e Llama 3.2 3B, medidos em 2026-11-01, veja [[modelos]]). Falta um cluster de 3 a 4 máquinas e o runbook de operação. | hardware |
| Aceleradores e storage | Medir kernels de GPU/NPU; SMART, erros e latência de escrita; capacidades em x86 e ARM64. | hardware |
| ADR da fase F03 | Ver se alguma decisão pede ADR. | decisão |

## F04 — Escalonamento

| Item | O que falta | Dependência |
|---|---|---|
| Custo do atestado e das reservas | `require_attested` refaz os atestados (O(workers)) e o ranking custa O(unidades × workers) sob o lock. Sem medição de gargalo: com 2 a 4 workers é desprezível, e a gravação do snapshot já espera um intervalo adaptativo. Só mexer com perfil em mãos (dezenas de workers). | dados |
| Níveis de confiança | O token de uso único por worker e a aprovação por regra entram com a etapa de espera ([[adr-023-instalacao-com-aprovacao-do-controller]]), sobre o [[adr-020-niveis-de-confianca-do-no]]. | decisão, código |
| Scheduler por GPU/NPU | Filas por recurso; hoje só CPU. | hardware |
| Localidade | Entra no custo só quando as saídas ficarem residentes no worker (F12). | código |

## F06 — Estado e recuperação

| Item | O que falta | Dependência |
|---|---|---|
| Estado por chave | KV cache, estado de execução e de modelo não existem no cluster (depende de F12). | código |
| Replicação da fila | A cópia fica num worker só: faltam fator de replicação, portador designado e identidade de cluster. | código |
| Checkpoint de unidade e `state://` | Retomar no meio do cálculo; referências lógicas de estado. | código, decisão |

## F07 — Segurança base

| Item | O que falta | Dependência |
|---|---|---|

## F08 — Inferência

| Item | O que falta | Dependência |
|---|---|---|
| Tolerância a falha | Só reatribuição: sem réplica de fatias pré-carregada nem rebalanceamento quando um worker entra; o teto de operações em voo por worker (64, `AI_CLUSTER_SHARD_MAX_PENDING`) dá 503 ao passar. | código |
| Engine fora do scheduler | O canal de fatias ainda passa pelo controller. Tirá-lo exige porta de entrada no worker (revisa o [[adr-009-canal-residente-de-fatias]]) para um motor que só serve à paridade e à pesquisa; reabrir se o motor próprio voltar a ser caminho de uso. | decisão |

## F09 e F10 — Transporte e storage

| Item | O que falta | Dependência |
|---|---|---|
| Cluster sobre o `Transport` | O cluster chama o gRPC direto; TCP e Unix existem fora do caminho. | código |
| E3, E5 | Memória compartilhada sem implementação; perda e congestionamento contínuo (`TCP_INFO`/netlink). | código |
| E4, USB | Vai para F18. | hardware |
| Storage | Alocação dinâmica por classe (swap, cache de modelo e tensor, checkpoint); dashboard; adaptação por desgaste. (A sonda de disco lê com `O_DIRECT` de propósito, para medir o disco e não a RAM; isso não é pendência de desempenho.) | código |
| Imagem do Orange Pi | A imagem 11.8 tem o worker antigo, cuja sonda de disco mede `/var/tmp` em RAM (SD com score 0,80 em vez de 0,01). O conserto está no código; falta gerar e gravar uma imagem nova. | código, hardware |
| Disco real | SMART/NVMe, políticas HDD × NVMe, SSD como cache ou swap. | hardware |

## F11 — Request Manager

| Item | O que falta | Dependência |
|---|---|---|
| Estados, idempotência, orçamento | Não existem. | código |
| Equidade e backpressure | Os tetos de admissão são configuráveis (`AI_CLUSTER_MAX_IN_FLIGHT`, `AI_CLUSTER_QUEUE_MAX`, `AI_CLUSTER_QUEUE_WAIT_SECS`), mas fixos por processo e iguais para todos os tenants. Falta equidade entre tenants sob carga. | código |
| Deadlines | Os do `ShardMatmul` (`AI_CLUSTER_SHARD_TIMEOUT_MS`) e do gateway (`AI_CLUSTER_GATEWAY_DEADLINE_MS`) são configuráveis, mas fixos por processo. Falta derivar do tempo medido de cada worker. | código |
| Entrada arbitrária no grafo | Hoje só o grafo sintético e os modelos fixos. | código |
| Fora do gateway | O Request Manager ainda mora no gateway. | código, decisão |

## F12 — Model runtime distribuído

O fluxo desejado (pasta de modelos no controller, OK/NOK, plano de workers, chaves e benchmark, worker novo) está feito para modelos GGUF no llama.cpp (veja [[model-manager]] e o [[adr-022-fluxo-de-modelos-e-motor-hibrido]]). As linhas abaixo são o que falta ou o que só vale para o motor próprio.

| Item | O que falta | Dependência |
|---|---|---|
| N1 a N3 | Catálogo por pasta, quantização (GGUF) e OK/NOK existem; falta manifesto assinado. O motor próprio segue só em fp32. | código, decisão |
| Admissão por capacidade | Falta barrar por folga de failover (hoje só informa), estimar o tempo por token sem um modelo de referência rodando. | código |
| Tensor com all-reduce e planejador híbrido | Não existem. O pipeline próprio foi descartado por medição ([[adr-022-fluxo-de-modelos-e-motor-hibrido]]); o llama.cpp reparte camadas pelo RPC e empurra os tensores na carga. Só reabrir com workers de velocidade parecida em cabo. | dados |
| Execução fora do gateway | O placement é do controller (`models::select`). O modelo ainda roda no processo do gateway, e o KV não é distribuído nem persistente. | código |

## F13 e F14 — Graph engine e edge optimizer

| Item | O que falta | Dependência |
|---|---|---|
| Graph engine (L1 a L4) | Dados, projeção, travessia e benchmark. Só adotar com ganho medido. | dados |
| Edge optimizer (M2 a M10) | Cache semântico, recuperação por grafo, contexto, deduplicação, compressão, skills, compilador de prompt, orçamento de tokens, caminho rápido; medir o ganho de cada etapa (só o M1 foi medido). | código, dados |

## F15 e F16 — SO e aceleradores

| Item | O que falta | Dependência |
|---|---|---|
| Console no monitor | Ver o `aic-console` e `F2`/`F12` no HDMI do Orange Pi com teclado real. | hardware |
| J1, x86 real | Firmware de rede e vídeo, watchdog, redimensionar o ext4 num disco real, Secure Boot, PXE. | hardware |
| J2 a J4 | Imagem de SO além do contêiner, `bootstrap.sh` em VM com systemd, reimage automático pelo controller. | código, hardware |
| J5, J6, assinatura de boot | A/B em mais placas, ARM64, u-boot que verifique a imagem (TPM/secure boot). | hardware |
| Modo dedicado no instalador | Controller e gateway dentro do AI Cluster OS (hoje só o worker tem imagem). O papel vira configuração em `/data`. O critério do slot do controller está no código (`slot-ready.sh`: o serviço respondeu na porta, testado por script); falta usá-lo quando a imagem do controller existir. Validar a imagem x86 em hardware real. | código, decisão, hardware |
| Instalação com aprovação do controller | O registro de pendentes, a aprovação com o disco a apagar e o token de uso único existem no controller e na CLI (`aic cluster install list`, `approve`, `reject`). O `aic-wait` (coleta o resumo, mostra a impressão digital, pergunta de novo) existe e foi testado contra o controller (`phase-u-wait.sh`). O `ClaimInstall` consome o token de uso único, pré-aprova o id e entrega a URL e o join token, e `aic-wait --claim` grava o `worker.env` (600). Feito sem hardware: o ambiente de instalação (`os/image/x86/build-installer.sh` e `aic-install.sh`: espera pela aprovação, download conferido, escolha de disco, claim e gravação com `worker.env`; `phase-u-x86-installer.sh`). Feito sem hardware (`phase-u-usb.sh`): o pendrive `usb.zip` pelo navegador (GRUB que acha `/aic/grub.cfg` em qualquer disco, sem rótulo, `BOOTX64.EFI` do build), o controller servindo a imagem e o `SHA256SUMS` assinados com a chave de Linux, e `forced` propagado ao instalador. Falta do [[adr-023-instalacao-com-aprovacao-do-controller]]: o boot desse pendrive em PC real com o cfg embutido novo (em QEMU/OVMF funciona; o anterior caía em `grub>` por `syntax error`), netboot, a atualização de um controller já instalado trazer `dedicado/` (hoje só instalação nova copia), validar tudo em hardware, TLS com a CA do controller fixada na imagem (hoje o join token também sai em claro sem TLS), o reinício do Linux existente para a imagem de espera (o `/wait` do convite já apresenta o aparelho sem reiniciar), pendrive e netboot sem lista de MACs obrigatória (MAC como pré-aprovação opcional), e auditoria do claim (hoje só o `Announce` da aprovação fica registrado). Substitui "Boot de rede restrito" e "Instalador em pendrive". | código |
| Release em mais processadores | ARM de 64 bits e, depois, celular e desktop: o `install.sh` já recusa com mensagem clara. Gateway em ARM exige compilar o `llama-server`. | código, hardware |
| Inglês na CLI e nos binários | O instalador, os comandos de instalação e atualização (`--lang en`) e o `bootstrap.sh` falam inglês, mas a CLI `aic`, o `aic-wait` e os logs do controller, gateway e worker só em português. O seletor único (`AIC_LANG`, em `ai-cluster-common::idioma`) já existe e a ajuda do `aic` obedece; faltam as mensagens traduzidas (texto novo usa `idioma::t(pt, en)`). | código |
| Base Linux | Escolher a base específica. | decisão |
| Aceleradores | ARM64, GPU NVIDIA/AMD/Intel, NPU (abstração e móvel); o worker não preenche o campo de aceleradores; scores por workload (M6). | código, hardware |

## F17 e F18 — Adaptativo e móvel

| Item | O que falta | Dependência |
|---|---|---|
| P1 a P6 | Modelo aprendido que persista e dependa do tamanho, placement preditivo, prefetch, execução especulativa, inferência de capacidade, rebalanceamento proativo. Saída: previsão melhor em workload repetido. | código, dados |
| Worker móvel (Q1 a Q6, M7) | Matriz de hardware, bootloader e reimage, imagem ARM64, USB, acelerador móvel, bateria real (hoje sensor simulado). | hardware: celulares |
| Wi-Fi no AI Cluster OS | Decidido que entra (celulares só terão Wi-Fi, provavelmente). Hoje a imagem não traz firmware nem `wpa_supplicant`, e o `setup` só configura Ethernet por DHCP. Falta: ADR; `wpa_supplicant`/`iw` e firmware por chipset nas imagens ARMv7 e x86 (hoje só `linux-firmware-rtl_nic`); credencial de rede (SSID e senha WPA2/WPA3) coletada no convite ou na tela de instalação e guardada em `/data` sem aparecer em log; `networking` subindo a interface sem fio e caindo para o cabo; adaptador USB de referência para o Orange Pi PC (sem Wi-Fi na placa); túnel mTLS e cache de tensores calibrados para enlace instável e de menor vazão (o Pi a 100 Mb/s já atrasa 25 a 33%); teste de reconexão após queda do AP; plano que desconte worker em Wi-Fi. Adaptador de referência: TP-Link TL-WN821N (USB 0cf3:7015, Atheros AR9287, driver `ath9k_htc`, firmware `htc_7010.fw`). O kernel do Pi (4.19.20-sunxi) traz cfg80211 e mac80211 como módulo, mas não tem `ath9k_htc` nem `/lib/firmware`: o aparelho é reconhecido no USB e fica sem interface `wlan0`. Falta compilar `ath9k_htc` (+ `ath9k_common`/`ath`) no kernel da imagem ARMv7 e incluir o firmware. Não testado em hardware. **Feito (sem hardware, verificado por `code/tests/os-wifi-overlay.sh` e testes Rust):** `wpa_supplicant`/`iw`/`wireless-regdb` nas imagens ARMv7 e x86, `linux-firmware-ath9k_htc` no x86, `aic worker wifi set|status|scan|clear` (`/data/wifi/wpa_supplicant.conf` 0600, senha por stdin), `wlan0` com métrica 200 e `netwatch` (cabo preferido, DHCP do cabo refeito ao voltar), campo `link` em `CapabilityReport` e desconto de 0,5 na taxa de transferência do plano (provisório até o Lote 9). **Falta:** (a) kernel do Pi: **não precisa recompilar nem trocar o `zImage`.** O `.config` embutido no próprio `zImage` (IKCONFIG) já tem `CONFIG_ATH9K_HTC=m`, `ATH9K_COMMON/HW`, `ATH_COMMON`, `CFG80211`, `MAC80211`, `RFKILL` como módulo, sem `MODVERSIONS` nem `MODULE_SIG`; o que falta é o rootfs Alpine não ter `/lib/modules` (os módulos ficavam no Armbian da placa). O kernel mora na FAT `AICBOOT` fixa (`mk-disk-image.sh:20,36`; `boot.cmd:50,89` carrega `zImage`/`dtb` de `mmc 0:1`), fora dos slots; o `aic-updater` só grava a partição do slot inativo (`aic-updater:71-81`). Logo, trocar o kernel exige regravar o cartão, mas **módulos + firmware dentro do rootfs entram por atualização assinada**. `code/os/image/build-kernel-wifi.sh` compila (Ubuntu 18.04, gcc 7, `linux-4.19.20` do kernel.org com o `.config` extraído do `zImage`, `LOCALVERSION=-sunxi`) `rfkill`, `cfg80211`, `mac80211`, `ath`, `ath9k_hw`, `ath9k_common`, `ath9k_htc` e pega `htc_7010.fw` do Alpine 3.20; saída em `.tmp/kernel-wifi/out/lib/{modules/4.19.20-sunxi,firmware}` (feito, `modinfo`: vermagic `4.19.20-sunxi SMP mod_unload ARMv7 thumb2 p2v8`, igual ao do `zImage`; alias `usb:v0CF3p7015`). Tamanho extra: 1,9 MiB descompactado (~0,8 MiB gz; módulos 1,93 MiB + firmware 71 KiB). Risco: fonte vanilla, sem os patches do Armbian (não há tag da época), então o layout de structs não foi comparado byte a byte. **Feito (script, sem hardware):** `build-rootfs.sh` copia `out/lib` para `wifi-lib/` (aviso e imagem sem Wi-Fi se faltar), o Dockerfile faz `COPY` em `/lib`, `depmod -b / 4.19.20-sunxi` e `ath9k_htc` em `/etc/modules`; `aic worker wifi status` mostra o driver (ausente, presente ou carregado); `os-wifi-overlay.sh` cobre. **Falta para testar no Pi:** gerar a imagem, conferir `uname -r` do Pi = `4.19.20-sunxi`, e na bancada ver `dmesg` (`insmod` sem `Invalid module format`) e `wlan0`; `regulatory.db` exige a chave embutida (`CFG80211_REQUIRE_SIGNED_REGDB=y`), conferir `iw reg get`. (b) pedir SSID e senha no convite (`aic worker join`/`setup`) e na tela do instalador x86 (`aic-install.sh`), validando com DHCP de teste; (c) o console mostrar `/run/aic-wifi.status` sem `clear`; (d) o `iw reg` e a lista de firmware de outros chipsets (`ath10k`, Lote 8); (e) todos os testes de hardware do ADR-030. | decisão, código, hardware: adaptador Wi-Fi, kernel do Pi | **Diagnóstico no Pi (03/11/2026, 0.3.2, TL-WN821N a -59 dBm, AP WPA2/WPA3 JuliansNET):** com a senha certa o handshake de 4 vias vai até o fim (RX 1/4, TX 2/4, RX 3/4, TX 4/4, o que prova o PSK), e então `nl80211: set_key failed; err=-2 (ENOENT)` ao instalar a PTK CCMP faz o `wpa_supplicant` desautenticar (Reason 1, "WPA: Failed to set PTK"). Igual com `nohwcrypt=1` no `ath9k_htc`, então o ENOENT vem do mac80211 (estação ausente no momento da chave), não da chave por hardware. Descartados: sinal, senha, PMF/SAE e o `netwatch` (que já não recicla a interface). Suspeita: `cfg80211`/`mac80211`/`ath9k_htc` compilados do linux-4.19.20 vanilla, sem os patches do Armbian sunxi. Próximo passo: obter o fonte do Armbian 4.19.20-sunxi e recompilar os módulos, ou testar o `ath9k_htc` com o `mac80211` do próprio kernel. Também: `rc-service aic-wifi stop` não encerra o `wpa_supplicant` (o pidfile não casa), o que acumula instâncias. **Causa provável achada por leitura de código (03/11/2026, sem Pi): falta `ccm.ko` no rootfs.** `ieee80211_add_key` (net/mac80211/cfg.c:392) aloca a chave antes de procurar a estação; `ieee80211_key_alloc` (key.c:443-448) chama `crypto_alloc_aead("ccm(aes)")` (aead_api.c:94), e o `.config` do zImage tem `CRYPTO_CCM=m`/`GCM=m`/`GHASH=m` (CTR/AES/CMAC y) enquanto o rootfs Alpine só tem os 7 módulos Wi-Fi: sem o template o crypto devolve -ENOENT na PTK. Explica handshake completo (EAPOL em claro) e o mesmo erro com `nohwcrypt=1`. Hipótese Armbian **descartada**: os patches de armbian/build@f82c047 (sunxi-next, 25/02/2019) não tocam net/mac80211, net/wireless nem ath9k além dos `patch-4.19.x-y` stable (idênticos ao vanilla); `.config` x `olddefconfig` só difere em símbolos sem relação; wpa_supplicant 2.10/Extended Key ID não se aplica (4.19 não anuncia). Artefato (sem rollout): `build-kernel-wifi.sh` agora também compila `crypto/{ccm,gcm,ghash-generic}.ko` em `.tmp/kernel-wifi/out/lib/modules/4.19.20-sunxi` (SHA-256 prefixo: *ccm.ko 137749e6b1f14bce; *gcm.ko 9ada09a2bb875a21; *ghash-generic.ko e318a7bf80710738; vermagic igual ao zImage). **Plano com cabo no Pi (barato primeiro):** (1) `grep -E 'ccm|gcm' /proc/crypto` e `dmesg | grep -iE 'ccm|failed to move'`; (2) `insmod ccm.ko` (+ `ghash-generic.ko gcm.ko`), reiniciar o `wpa_supplicant`, ver `wpa_cli status` COMPLETED; (3) se ainda ENOENT: `iw dev wlan0 station dump` e `iw event -t` logo após associar (estação ausente/sem ASSOC, cfg.c:415; `failed to move station` indica `ath9k_htc_sta_add`/WMI); (4) por último `ieee80211w=0`/sem SAE e wpa_supplicant 2.9. Sem `DYNAMIC_DEBUG` nem `MAC80211_DEBUG_MENU` no kernel, não há trace. Se confirmado: Dockerfile do Pi põe `ccm` (e `ghash-generic gcm`) em `/etc/modules`/`modules.dep` antes do `ath9k_htc` e a imagem 0.3.3 leva os `.ko`; atualização assinada basta (rootfs).
| Instalação por pendrive via Wi-Fi | Hoje o instalador (initramfs) só pede DHCP por Ethernet e não chega ao controller sem cabo. Falta: módulos e firmware do adaptador (`ath9k_htc` no ARMv7, firmware por chipset no x86) e `wpa_supplicant`/`iw` no initramfs; tela do instalador (`aic-wait`/`aic-install.sh`) pedindo SSID e senha WPA2/WPA3 (sem eco, nunca por argumento, log, `ps` nem console); passar a credencial ao sistema instalado (gravada em `/data/wifi/wpa_supplicant.conf`, 0600, no primeiro boot) e a impressão digital e a aprovação seguirem pelo Wi-Fi; Pi: o `zImage` fixo na FAT `AICBOOT` já tem de aceitar os módulos; falha clara quando o SSID não conecta, sem piscar a tela. Decisão tomada em 02/11/2026: entra depois do teste de Wi-Fi no sistema instalado (Lote 9), porque a atualização assinada já cobre quem está instalado. Não testado em hardware. | código, hardware: adaptador Wi-Fi e AP |

## F19 — Hardening de produção

| Item | O que falta | Dependência |
|---|---|---|
| Revogação | Lista de revogação de chaves de release do AI Cluster OS e de certificados de worker (a da chave de Linux comum está pronta; o controller já não renova nem anuncia o worker revogado, ADR-026; falta CRL para quem não é o controller); revogação e rotação de tokens de operador e de recuperação (uso único, ADR-017). Worker revogado que se reinicia entra pendente com credencial nova; só `remove` põe lápide (ADR-027). Lápide que sobreviva à reinstalação do controller. | código, decisão |
| mTLS | Ligar o certificado ao `node_id`; rotação, CRL e emissão automática; TLS no exportador de métricas. | código |
| Rollout | HTTPS no convite (o servidor da porta 8099 segue em HTTP, protegido por impressão de chave e hash; o download da release e do cache já é só HTTPS quando a origem é https, sem seguir redirecionamento para HTTP); marca guarda uma versão só; downgrade legítimo com bandeira assinada na release, limitada a uma versão que o worker já rodou (o roll-forward é a regra, ADR-031; a exceção pede mudar o `aic-updater` da imagem e a marca, então entra no Lote 4 e só se o primeiro rollback real pedir); ondas com mais de um worker. | código, decisão, hardware |
| Integridade de boot | dm-verity com root hash assinado; TPM e secure boot; medida de 512 MiB leva 25 a 40 s no SD. | hardware |
| Auditoria | O log persiste, mas não é assinado nem à prova de adulteração. | código, decisão |
| Saúde do upgrade | Binário que registra e quebra depois passa; sem rollback sem systemd. | código |

## aic, instalação e atualização

| Item | O que falta | Dependência |
|---|---|---|
| Boot completo da imagem x86 do SO | A imagem x86 UEFI foi reconstruída com o `aic` (12.1), o userland foi conferido (`aic version`, `aic update status`, `aic --help`, helpers em `/usr/libexec/aic`, nenhum comando antigo) e ela bootou no QEMU+OVMF até o OpenRC. O boot inteiro não terminou: sem KVM neste PC o QEMU roda em TCG e o boot passou de 30 min com soft lockups em `fork`/`vmalloc`, sem chegar ao registro, ao ssh nem ao rollout x86. Hipótese não verificada: o `aic-measure` (SHA-256 de 512 MiB do slot, antes do worker) é lentíssimo em TCG; testar sem ele ou com `-cpu` simples, ou em máquina com KVM ou em hardware real. A imagem x86 embute só `release.pub` (o ARM, `nova.pub`): o rollout x86 assina com `release.key`. | hardware, código |
| Gatilho de atualização para controller e gateway Linux | Não é empurrado: cada um roda `aic update apply`, já com a assinatura conferida. Empurrar exigiria um canal de comando no gateway, sem ganho sobre o pino ([[adr-025-gatilho-de-atualizacao-assinada-linux]]). Reabrir se o operador precisar do empurrão. | decisão |
| Gatilho com systemd de verdade | Os units `ai-cluster-update.path` e `.service` são escritos pelo `setup` e os testes os aplicam à mão (systemd de mentira); nunca rodaram com um systemd real. Falta validar num Debian com systemd, e o vigia sem systemd ([[adr-025-gatilho-de-atualizacao-assinada-linux]], item 8) num OpenRC de verdade (a imagem de teste é Debian sem init). | hardware, código |

## Marcos

M0 (duas máquinas), M2 (máquina limpa vira worker), M3 (storage), M6 (x86 e ARM) e M7 (celular) dependem de hardware. M4 pede o overhead de comunicação decomposto no trace e os tokens esperado e real. M5 depende de F11, F12 e F13.

## llama.cpp por RPC (ADR-022)

| Item | O que falta | Dependência |
|---|---|---|
| Certificado por worker | Renovação e revogação prontas (ADR-026): o controller guarda a CA e renova o certificado do túnel de cada worker pela credencial do worker, com chave fixada por worker e escrita atômica no worker. Falta: rotação da CA; trocar a chave do worker na renovação; CRL para quem não é o controller; worker esquecido ou revogado offline volta só com novo convite (regra de operação, sem imposição no código); entrada de worker novo, que ainda passa pelo convite; sem systemd e em instalação antiga a renovação fica desligada; renovação ainda não exercitada ponta a ponta nem em hardware. | código, decisão |

## Ideias do distributed-llama (análise de 01/11/2026)

Todas entram sem refactor: usam o llama.cpp por RPC, o plano e o gateway que já existem. Ordem de execução; esforço P/M/G. Fonte: leitura do repositório b4rtaz/distributed-llama (README, docs e `src/`); o código de sincronização do ggml-rpc não foi lido a fundo e as medições deles em Raspberry Pi (Discussions) não foram consultadas.

| # | Item | Esforço | Condição |
|---|---|---|---|
| 1 | Medir cálculo x sincronização por token (TTFT/ITL já vêm do `timings`; falta o tempo de rede por token pelo túnel). Base para decidir o item 3 e o marco M4. | P/M | nenhuma |
| 3 | Quantizar as ativações enviadas pelo RPC (~3,5x menos bytes por salto). Exige patch em `third_party/llama.cpp` e teste de paridade. | M | só se o item 1 mostrar a rede como gargalo (Pi a 100 Mb/s) |
| 4 | Contar o gateway como worker de cálculo no plano (llama.cpp aceita backend local + RPC). Adiado (2026-11-02), não é mudança pequena: o gateway não é worker registrado, então o controller não conhece a RAM dele; e o `--tensor-split` só reparte entre dispositivos (os `--rpc`), a fatia local do CPU vem do `-ngl` menor que o total de camadas, o que pede o número de camadas do GGUF e um cálculo novo no plano e no gateway. Pede campo novo no plano (`DesiredModel`) e RAM reportada pelo gateway; fazer junto do Lote 2, com medição. | P/M | RAM do gateway sobrando |
| 5 | Vulkan no build x86 (`-DGGML_VULKAN`), com medição. | P/M | só x86 com GPU integrada; não serve ao ARMv7 |
| 6 | ~~`aic cluster model pull`~~ feito para `.gguf` (URL https ou `dono/repo/arquivo.gguf`, tamanho, assinatura e SHA-256 opcional). Tokenizador: o GGUF já o traz. Falta o SHA-256 automático do Hugging Face (`x-linked-etag`) e um comando que peça ao controller baixar (hoje roda no computador dele). | M | nenhuma |
| 7 | QUICKACK no túnel (TCP_NODELAY já existe em `crates/tunnel`). | P | medir ganho antes |
| 8 | Paralelismo de tensor por camada. Só compensa em rede de 1 Gb/s com 2 a 4 workers; em 100 Mb/s a sincronização por camada domina. Reavaliar junto do F12. | G | cluster gigabit de 2 a 4 workers |

Não copiar: fatias iguais e número de workers em potência de 2 (limite deles, ver ADR-022), formato próprio `.m`/`.t`.

## Modo dedicado x86: ajustes do primeiro teste real

| Item | O que falta | Dependência |
|---|---|---|
| Texto maior/centralizado na tela do instalador e do SO | Feito no SO (`tty_font`, Terminus pelo `setfont`, testado por script); vale na próxima imagem. Falta a tela do instalador (initramfs), que precisa de fonte no initramfs, e ver no monitor real | nova imagem x86, hardware |
| Textos do console F2 (reiniciar o serviço do worker) | Já no código (menu "Reiniciar o servico do worker"); só vale na imagem de disco reconstruída (build-rootfs.sh + mk-disk.sh) | nova imagem x86 |
| `phase-u-usb.sh` no Windows | Falha por caminho `/d/...` no Python do Windows (ambiente, não o código) | rodar em Linux/WSL |

## Versão única (ADR-029)

- Feito: os dois `build-rootfs.sh` apagam `build/rootfs-<número>*` (numeração antiga) a cada build; os arquivos atuais (`rootfs.ext4`, `rootfs-x86.ext4`) não casam. Nada foi apagado à mão.

## Worker instalado pelo claim x GRUB da imagem com identidade (varredura 2026-11-01)

- Sem TLS no modo dedicado: o worker instalado só fala com controller `http://` (`AI_CLUSTER_INSECURE=1`); o claim não entrega a CA do join (TLS do controller), fica para a etapa 7 (CA fixada no instalador). Adiado em 02/11/2026: o controller não conhece a CA do próprio certificado TLS (vem de `AI_CLUSTER_TLS_CERT`), então a CA precisa ser fixada fora de banda (no `usb.zip`/convite, junto da chave de Linux) e o `wait`/claim usá-la já no primeiro contato; isso muda `ClaimInstall` (campo novo é compatível), o `mkusb`, o `install-disk` e só se valida com um controller https e um aparelho.

- **Fonte única da versão (achado de 01/11/2026).** `aic controller status` e `aic gateway status` mostravam 0.2.1 e o slogan antigo ("hardware comum") com os binários já na 0.2.3: o `setup` do lab era uma cópia velha e `versao()` lia o `VERSION` do arquivo `/etc/ai-cluster/release`, só um registro da instalação. Corrigido: `versao()` pergunta ao binário (`aic version --short`, nova opção) e só cai no arquivo se não houver binário. Falta um único caminho de deploy do lab (hoje as cópias de `setup`/`bootstrap.sh` são manuais e se perdem); o certo é o lab usar o próprio `install.sh`/`setup update`.

## Hardware real: o que ainda falta

O histórico dos testes, as medições e as causas estão em [[diario-de-hardware]]. **Regra de validação:** versão nova entra nos aparelhos só por atualização assinada (rollout, F2 ou `aic update`); pendrive só para recuperar aparelho que não sobe ou dispositivo novo.

- **Dell (notebook): "Hard drive not installed"** depois do rollout 0.2.3 (tela de recuperação da Dell): precisa de uma pessoa na tela para diagnosticar.
- **Sem teste em hardware:** vazão da rede nos dois sentidos e sob carga; temperatura do Pi (`TEMP_C` 0: o kernel do Pi não traz o driver `SUN8I_THS` do Armbian); Pi no limite de RAM e térmico; perda de energia no meio do A/B; Secure Boot; PXE; pendrive em PC real; reconexão do Wi-Fi após queda do AP.
- **Quedas dos notebooks após rollout (0.2.4):** a causa provável era o serviço do worker ficar `failed` no openrc (corrigido com `respawn_max=0`); nos rollouts 0.3.3 e 0.3.4 nenhum notebook caiu. Manter observação.
- **Updater:** falha de download não deveria bloquear a versão para sempre (hoje `update-tried` impede repetir).
- **Gateway:** se o `llama-server` recusar a flag do modelo auxiliar (versão antiga) ou o vocabulário divergir, o supervisor reinicia em loop; deveria voltar a gerar sem o auxiliar.
- **Pi:** conferir NEON e produto escalar no `rpc-server` (armv7, `-mcpu=cortex_a7`); sensor térmico.
- **Instalação por pendrive via Wi-Fi:** não implementada (celulares só terão Wi-Fi).
