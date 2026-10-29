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
| Custo do atestado e das reservas | `require_attested` refaz os atestados (O(nós)) e o ranking custa O(unidades × nós) sob o lock; `reserve`/`release` gravam o snapshot sob o lock. | código |
| Níveis de confiança | Enrollment (token de uso único, aprovação por regra) sobre o [[adr-020-niveis-de-confianca-do-no]]. | decisão, código |
| Scheduler por GPU/NPU | Filas por recurso; hoje só CPU. | hardware |
| Localidade | Entra no custo só quando as saídas ficarem residentes no worker (F12). | código |

## F06 — Estado e recuperação

| Item | O que falta | Dependência |
|---|---|---|
| Estado por chave | KV cache, estado de execução e de modelo não existem no cluster (depende de F12). | código |
| Replicação da fila | A cópia fica num nó só: faltam fator de replicação, portador designado e identidade de cluster. | código |
| Journal | O snapshot reescreve o estado inteiro a cada gravação; falta journal de deltas. O `claim` sob o lock global também grava o estado todo (sem fsync). | código |
| Checkpoint de unidade e `state://` | Retomar no meio do cálculo; referências lógicas de estado. | código, decisão |

## F07 — Segurança base

| Item | O que falta | Dependência |
|---|---|---|
| Nó desconhecido | Um `node_id` fora do registro passa pela checagem de credencial. | código |
| Escopo do operador | O operador age sobre qualquer nó. | decisão, código |
| Atalho `--trust-listed` | Aprova a impressão que o controller mostra, sem comparar com a do nó; anula a proteção contra o id tomado. Usado nos scripts de teste e no bootstrap manual. | decisão |
| Gateway atrás de NAT | O id é amarrado ao IP; atrás de NAT todos parecem um só, e quem troca de IP espera 45 s. Falta limite de ids por IP e segredo por gateway. | código, decisão |
| Segredo lido uma vez | `secret()` relê o arquivo a cada chamada e, se o `_FILE` falhar, só avisa. Ler no início e falhar. | código |
| Relatório aberto | `WorkReport` e `ListEvents` aceitam o token de join; `limit` 0 devolve tudo sob o lock. Exigir operador ou teto. | código |
| Gateway sem TLS | `axum::serve` sem teto de conexões nem tempo de cabeçalho (o TLS tem os dois). | código |

## F08 — Inferência

| Item | O que falta | Dependência |
|---|---|---|
| Token escolhe nó | O token de inferência escolhe nó, subdiretório, tensor e faixa. | código |
| Tolerância a falha | Só reatribuição: sem réplica de fatias pré-carregada nem rebalanceamento quando um nó entra; mais de 64 operações em voo por nó dão 503. | código |
| Quarentena | Derruba a sessão de inferência (contenção de segurança, por desenho); falta devolver ao cliente um erro limpo (503 com motivo). | código |
| Engine fora do scheduler | O canal de fatias ainda passa pelo controller. | código |

## F09 e F10 — Transporte e storage

| Item | O que falta | Dependência |
|---|---|---|
| Cluster sobre o `Transport` | O cluster chama o gRPC direto; TCP e Unix existem fora do caminho. | código |
| E3, E5 | Memória compartilhada sem implementação; perda e congestionamento contínuo (`TCP_INFO`/netlink). | código |
| E4, USB | Vai para F18. | hardware |
| Storage | Leitura sem cache de página; alocação dinâmica por classe (swap, cache de modelo e tensor, checkpoint); dashboard; adaptação por desgaste. | código |
| Imagem do Orange Pi | A imagem 11.8 tem o worker antigo, cuja sonda de disco mede `/var/tmp` em RAM (SD com score 0,80 em vez de 0,01). O conserto está no código; falta gerar e gravar uma imagem nova. | código, hardware |
| Disco real | SMART/NVMe, políticas HDD × NVMe, SSD como cache ou swap. | hardware |

## F11 — Request Manager

| Item | O que falta | Dependência |
|---|---|---|
| Estados, idempotência, orçamento | Não existem. | código |
| Equidade e backpressure | Hoje há tetos fixos de admissão. | código |
| Cancelamento do ShardMatmul | O `ShardMatmul` em voo não recebe o sinal (as unidades de trabalho recebem pela batida). | código |
| Deadlines | Fixos: 30 s no gateway e 8 s no `ShardMatmul`. | código |
| Entrada arbitrária no grafo | Hoje só o grafo sintético e os modelos fixos. | código |
| Fora do gateway | O Request Manager ainda mora no gateway. | código, decisão |

## F12 — Model runtime distribuído

O fluxo desejado (pasta de modelos no controller, OK/NOK, plano de workers, chaves e benchmark, nó novo) está feito para modelos GGUF no llama.cpp (veja [[model-manager]] e o [[adr-022-fluxo-de-modelos-e-motor-hibrido]]). As linhas abaixo são o que falta ou o que só vale para o motor próprio.

| Item | O que falta | Dependência |
|---|---|---|
| N1 a N3 | Catálogo por pasta, quantização (GGUF) e OK/NOK existem; falta registro central com versão e variantes e manifesto assinado. O checksum só cobre a primeira parte de um GGUF dividido. O motor próprio segue só em fp32. | código, decisão |
| Compatibilidade de arquiteturas | Faltam viés de atenção e de MLP (Qwen), MoE, janela deslizante, ativações além de `silu` e a paridade contra o `transformers` com pesos de um Llama 3.x (veja [[inference-engine]]). `lm_head` fica em f64 (4 GB num 8B). | código, dados |
| Admissão por capacidade | Falta barrar por folga de failover (hoje só informa), estimar o tempo por token sem um modelo de referência rodando. | código |
| N4 a N9 | Pré-carga de modelo nos workers; limpeza das fatias de modelos antigos no cache do `rpc-server` (o cache é opcional e não apaga nada); medir o ganho do cache com um modelo grande (num Orange Pi, SD e um modelo de 0,37 GB não mostrou ganho: 51 a 67 s sem cache, 56 a 61 s com, e o cache guardou só 138 MB; o SD escreve ~5 MB/s, tão lento quanto a rede), decidir ligar o cache pela razão entre a leitura do disco e a banda da rede, em vez de uma regra fixa, pipeline com porta de entrada própria entre workers, tensor com all-reduce e planejador híbrido não existem: o llama.cpp reparte camadas pelo RPC e empurra os tensores na carga. | código |
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
| Imagem fora da release | Fluxo documentado para gerar a imagem com a própria chave. | código, documentação |
| J1, x86 real | Firmware de rede e vídeo, watchdog, redimensionar o ext4 num disco real, Secure Boot, PXE. | hardware |
| J2 a J4 | Imagem de SO além do contêiner, `bootstrap.sh` em VM com systemd, reimage automático pelo controller. | código, hardware |
| J5, J6, assinatura de boot | A/B em mais placas, ARM64, u-boot que verifique a imagem (TPM/secure boot). | hardware |
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
| Testes de resistência | Partição de rede, controller reiniciado na carga, soak de horas medindo memória. | código |
| Saúde do upgrade | Binário que registra e quebra depois passa; sem rollback sem systemd. | código |

## Marcos

M0 (duas máquinas), M2 (máquina limpa vira worker), M3 (storage), M6 (x86 e ARM) e M7 (celular) dependem de hardware. M4 pede o overhead de comunicação decomposto no trace e os tokens esperado e real. M5 depende de F11, F12 e F13.

## llama.cpp por RPC (ADR-022)

| Item | O que falta | Dependência |
|---|---|---|
| Certificado por nó | O papel do cliente (SAN `aic-gateway`) e o nome por nó (`{node_id}`) já valem; falta o controller emitir os certificados de nó (hoje a CA e os certificados são do operador) e renová-los. | código |
| Binários nas imagens | O `ggml-rpc-server` está nas imagens armv7 e x86 do SO, na imagem única do Compose (que leva também o `llama-server`) e no `bootstrap.sh` (`--rpc-server-bin`). Falta empacotar o binário para o worker em systemd (hoje o operador o fornece). | código |
