---
title: "Pendências"
aliases: ["O que falta", "Pendências do projeto"]
tags: [implementacao/pendencias]
type: implementacao/pendencias
---

# Pendências

Só o que falta, em poucas palavras. O que já existe está no [[guia]] e no sistema; o histórico está no git.

Dependência: **código** (resolve-se no repositório), **hardware** (máquina ou componente que o laboratório não tem: celulares, GPU/NPU, x86, ARM64, TPM, placa armv7 extra, duas máquinas físicas, disco real com SMART), **decisão** (escolha de projeto antes de codar) ou **dados** (corpus ou tarefa para medir).

## F00 a F03 — Fundação, núcleo, capability, execução

| Item | O que falta | Dependência |
|---|---|---|
| Duas máquinas físicas | Executar o runbook, o MVP distribuído (varredura com dois workers) e o cluster de 2 a 4 máquinas fora de contêiner. | hardware |
| Limiar térmico | Aquecer a placa até a saúde térmica cair (69 °C não bastou). | hardware: armv7 |
| Aceleradores e storage | Medir kernels de GPU/NPU; SMART, erros e latência de escrita; capacidades em x86 e ARM64. | hardware |
| ADR da fase F03 | Ver se alguma decisão pede ADR. | decisão |

## F04 — Escalonamento

| Item | O que falta | Dependência |
|---|---|---|
| Custo do atestado e das reservas | `require_attested` refaz os atestados (O(nós)) e o ranking custa O(unidades × nós) sob o lock. Sem medição de gargalo: com 2 a 4 nós é desprezível, e a gravação do snapshot já espera um intervalo adaptativo. Só mexer com perfil em mãos (dezenas de nós). | dados |
| Níveis de confiança | O token de uso único por nó e a aprovação por regra entram com a etapa de espera ([[adr-023-instalacao-com-aprovacao-do-controller]]), sobre o [[adr-020-niveis-de-confianca-do-no]]. | decisão, código |
| Scheduler por GPU/NPU | Filas por recurso; hoje só CPU. | hardware |
| Localidade | Entra no custo só quando as saídas ficarem residentes no worker (F12). | código |

## F06 — Estado e recuperação

| Item | O que falta | Dependência |
|---|---|---|
| Estado por chave | KV cache, estado de execução e de modelo não existem no cluster (depende de F12). | código |
| Replicação da fila | A cópia fica num nó só: faltam fator de replicação, portador designado e identidade de cluster. | código |
| Checkpoint de unidade e `state://` | Retomar no meio do cálculo; referências lógicas de estado. | código, decisão |

## F07 — Segurança base

| Item | O que falta | Dependência |
|---|---|---|
| Atalho `--trust-listed` | Aprova a impressão que o controller mostra, sem comparar com a do nó; anula a proteção contra o id tomado. Usado nos scripts de teste e no bootstrap manual. | decisão |
| Gateway atrás de NAT | O id é amarrado ao IP; atrás de NAT todos parecem um só, e quem troca de IP espera 45 s. O limite de gateways vivos por IP existe (`AI_CLUSTER_MAX_GATEWAYS_PER_HOST`); falta um segredo por gateway. | código, decisão |

## F08 — Inferência

| Item | O que falta | Dependência |
|---|---|---|
| Tolerância a falha | Só reatribuição: sem réplica de fatias pré-carregada nem rebalanceamento quando um nó entra; o teto de operações em voo por nó (64, `AI_CLUSTER_SHARD_MAX_PENDING`) dá 503 ao passar. | código |
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
| Deadlines | Os do `ShardMatmul` (`AI_CLUSTER_SHARD_TIMEOUT_MS`) e do gateway (`AI_CLUSTER_GATEWAY_DEADLINE_MS`) são configuráveis, mas fixos por processo. Falta derivar do tempo medido de cada nó. | código |
| Entrada arbitrária no grafo | Hoje só o grafo sintético e os modelos fixos. | código |
| Fora do gateway | O Request Manager ainda mora no gateway. | código, decisão |

## F12 — Model runtime distribuído

O fluxo desejado (pasta de modelos no controller, OK/NOK, plano de workers, chaves e benchmark, nó novo) está feito para modelos GGUF no llama.cpp (veja [[model-manager]] e o [[adr-022-fluxo-de-modelos-e-motor-hibrido]]). As linhas abaixo são o que falta ou o que só vale para o motor próprio.

| Item | O que falta | Dependência |
|---|---|---|
| N1 a N3 | Catálogo por pasta, quantização (GGUF) e OK/NOK existem; falta manifesto assinado. O motor próprio segue só em fp32. | código, decisão |
| Admissão por capacidade | Falta barrar por folga de failover (hoje só informa), estimar o tempo por token sem um modelo de referência rodando. | código |
| Tensor com all-reduce e planejador híbrido | Não existem. O pipeline próprio foi descartado por medição ([[adr-022-fluxo-de-modelos-e-motor-hibrido]]); o llama.cpp reparte camadas pelo RPC e empurra os tensores na carga. Só reabrir com nós de velocidade parecida em cabo. | dados |
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
| Modo dedicado no instalador | Controller e gateway dentro do AI Cluster OS (hoje só o worker tem imagem). O papel vira configuração em `/data`. O slot do controller não pode ser confirmado por "registrou no controller": falta o critério (por exemplo, o serviço respondeu na porta). Validar a imagem x86 em hardware real. | código, decisão, hardware |
| Instalação com aprovação do controller | O registro de pendentes, a aprovação com o disco a apagar e o token de uso único existem no controller e na CLI (`aic cluster install list`, `approve`, `reject`). O `aic-wait` (coleta o resumo, mostra a impressão digital, pergunta de novo) existe e foi testado contra o controller (`phase-u-wait.sh`). Falta do [[adr-023-instalacao-com-aprovacao-do-controller]]: colocá-lo na imagem de boot mínima (netboot e pendrive), uso do token na instalação, TLS com a CA do controller fixada na imagem, o reinício do Linux existente para a imagem de espera (o `/wait` do convite já apresenta o aparelho sem reiniciar), pendrive e netboot sem lista de MACs obrigatória (MAC como pré-aprovação opcional). Substitui "Boot de rede restrito" e "Instalador em pendrive". | código |
| Release em mais processadores | ARM de 64 bits e, depois, celular e desktop: o `install.sh` já recusa com mensagem clara. Gateway em ARM exige compilar o `llama-server`. | código, hardware |
| Inglês na CLI e nos binários | O instalador, os comandos de instalação e atualização (`--lang en`) e o `bootstrap.sh` falam inglês, mas a CLI `aic`, o `aic-wait` e os logs do controller, gateway e worker só em português. Falta um seletor único (`AIC_LANG`) no Rust e as mensagens traduzidas. | código |
| Base Linux | Escolher a base específica. | decisão |
| Aceleradores | ARM64, GPU NVIDIA/AMD/Intel, NPU (abstração e móvel); o worker não preenche o campo de aceleradores; scores por workload (M6). | código, hardware |

## F17 e F18 — Adaptativo e móvel

| Item | O que falta | Dependência |
|---|---|---|
| P1 a P6 | Modelo aprendido que persista e dependa do tamanho, placement preditivo, prefetch, execução especulativa, inferência de capacidade, rebalanceamento proativo. Saída: previsão melhor em workload repetido. | código, dados |
| Worker móvel (Q1 a Q6, M7) | Matriz de hardware, bootloader e reimage, imagem ARM64, USB, acelerador móvel, bateria real (hoje sensor simulado). | hardware: celulares |

## F19 — Hardening de produção

| Item | O que falta | Dependência |
|---|---|---|
| Revogação | Lista de revogação de chaves de release e certificados; revogação e rotação de tokens de operador e de recuperação (uso único, ADR-017). | código |
| mTLS | Ligar o certificado ao `node_id`; rotação, CRL e emissão automática; TLS no exportador de métricas. | código |
| Rollout | HTTPS no download (hoje HTTP, segue redirecionamento); marca guarda uma versão só; downgrade legítimo (decisão); ondas com mais de um nó. | código, decisão, hardware |
| Integridade de boot | dm-verity com root hash assinado; TPM e secure boot; medida de 512 MiB leva 25 a 40 s no SD. | hardware |
| Auditoria | O log persiste, mas não é assinado nem à prova de adulteração. | código, decisão |
| Saúde do upgrade | Binário que registra e quebra depois passa; sem rollback sem systemd. | código |

## aic, instalação e atualização

| Item | O que falta | Dependência |
|---|---|---|
| `aic update apply` no AI Cluster OS | Só explica que a atualização é empurrada pelo controller e pede ao `aic-updater` que cheque. Falta uma checagem local sob demanda. | código |
| Rollback do worker Linux | O `aic update apply` guarda o binário anterior em `/opt/ai-cluster/bin/.prev`, mas o caminho de volta não foi testado. | código |
| Boot completo da imagem x86 do SO | A imagem x86 UEFI foi reconstruída com o `aic` (12.1), o userland foi conferido (`aic version`, `aic update status`, `aic --help`, helpers em `/usr/libexec/aic`, nenhum comando antigo) e ela bootou no QEMU+OVMF até o OpenRC. O boot inteiro não terminou: sem KVM neste PC o QEMU roda em TCG e o boot passou de 30 min com soft lockups em `fork`/`vmalloc`, sem chegar ao registro, ao ssh nem ao rollout x86. Hipótese não verificada: o `aic-measure` (SHA-256 de 512 MiB do slot, antes do worker) é lentíssimo em TCG; testar sem ele ou com `-cpu` simples, ou em máquina com KVM ou em hardware real. A imagem x86 embute só `release.pub` (o ARM, `nova.pub`): o rollout x86 assina com `release.key`. | hardware, código |
| Ajuda do clap em inglês | A ajuda do `aic` ainda mostra `[possible values: pt, en]` em inglês, na opção `--lang`. | código |
| `aic worker change` sem `--token-file` | Só aceita `--token` na linha de comando, sem o token por arquivo que o `bootstrap.sh` tem. | código |
| Gatilho de atualização assinado para Linux | O controller já aciona a atualização dos nós com AI Cluster OS (a release assinada vai no heartbeat e o worker, com `AI_CLUSTER_UPDATE_CMD`, drena, sai e roda o comando). Para worker e gateway em Linux falta: (1) assinar o `SHA256SUMS` de cada release com a chave do operador e gravar a pública no `install.sh` e em `/etc/ai-cluster` (fecha o limite do `SHA256SUMS` sem assinatura e permite recusar versão mais antiga); (2) um comando de atualização do worker Linux que confere a assinatura e roda `aic update apply`, configurado como `AI_CLUSTER_UPDATE_CMD`; (3) o mesmo gatilho para o gateway, pelo canal que ele já usa com o controller. Decisão antes: qual chave assina os pacotes Linux (a das imagens do SO serve?). | código, decisão |
| Migração de servidor antigo | A atualização de um servidor instalado com a ferramenta de nome antigo (`setup update apply --force`, vindo do `tools.tar.gz`) não foi testada. | código |

## Marcos

M0 (duas máquinas), M2 (máquina limpa vira worker), M3 (storage), M6 (x86 e ARM) e M7 (celular) dependem de hardware. M4 pede o overhead de comunicação decomposto no trace e os tokens esperado e real. M5 depende de F11, F12 e F13.

## llama.cpp por RPC (ADR-022)

| Item | O que falta | Dependência |
|---|---|---|
| Certificado por nó | Cada worker gera a própria chave e o convite (`aic cluster invite`) assina o certificado, com o SAN `aic-worker-<nó>`. Falta renovar (825 dias), revogar e emitir sem convite aberto; a CA mora no servidor do convite, não no controller em si. | código |
