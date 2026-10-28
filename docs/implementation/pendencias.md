---
title: "Pendências"
aliases: ["O que falta", "Pendências do projeto"]
tags: [implementacao/pendencias]
type: implementacao/pendencias
---

# Pendências

Só o que falta. Para o que já existe, veja o [[guia]] e o sistema.

## Como ler

Cada item traz a dependência, que diz o que destrava o item:

- **código**: se resolve no repositório, com teste em contêiner ou unitário.
- **hardware**: exige máquina ou componente que o ambiente de desenvolvimento não tem. Os tipos aparecem assim:
  - *celulares*: aparelhos reais, com bateria e USB.
  - *GPU/NPU*: GPU NVIDIA, AMD ou Intel, ou NPU.
  - *x86*, *ARM64*: máquina física de cada arquitetura. (A placa armv7 já existe, em bancada.)
  - *TPM*: módulo TPM 2.0 e secure boot na placa.
  - *placa armv7*: placa com u-boot, em bancada, para boot, atualização A/B e recuperação.
  - *duas máquinas físicas*: dois nós independentes na mesma LAN.
  - *disco real*: HDD, NVMe ou SSD com SMART.
- **decisão**: falta uma escolha de projeto antes de codar.
- **dados**: falta um corpus ou uma tarefa para medir.

Os marcos de prova (M0–M7) ficam ao final.

## F00 — Fundação `[~]`

| Item | O que falta | Dependência |
|---|---|---|
| Runbook em duas máquinas | Executar o runbook em duas máquinas físicas independentes e registrar a tabela de execução. | hardware: duas máquinas físicas |
| MVP distribuído | Repetir a varredura com dois workers em máquinas separadas e comparar com adicionar máquina. A prova atual é em um host só. | hardware: duas máquinas físicas |
| Limiar térmico | Aquecer a placa até a saúde térmica cair. Não foi atingido (69 °C). | hardware: placa armv7 com carga térmica |

## F01 — Núcleo do cluster `[x]`

| Item | O que falta | Dependência |
|---|---|---|
| Cluster de 2 a 4 máquinas | Validar entrada e saída em máquinas físicas. Hoje só há containers. | hardware: 2 a 4 máquinas físicas |

## F02 — Capability e saúde `[~]`

Todo o código da fase está feito; o que resta depende de hardware que o laboratório não tem.

| Item | O que falta | Dependência |
|---|---|---|
| Relatório de trabalho | `completed()` ainda copia o histórico (até 110 mil registros) a cada `WorkReport`; devolver uma janela ou um iterador. As métricas já leem sem copiar. | código |
| Jitter ambíguo | `network_jitter` 0 vale "estável" e "não medido" (menos de duas repetições); precisa de um sinal separado. | código |
| Aceleradores | Reportar e medir kernels de GPU e NPU. | hardware: GPU/NPU |
| Saúde de storage | SMART, erros e latência de escrita por dispositivo. | hardware: disco real com SMART |
| Hardware heterogêneo | Validar capacidades distintas com mais tipos de máquina. | hardware: x86 e ARM64 |

## F03 — Execução distribuída `[x]`

| Item | O que falta | Dependência |
|---|---|---|
| ADR da fase | Confirmar se alguma decisão da fase precisa de ADR. | decisão |

## F04 — Escalonamento `[~]`

| Item | O que falta | Dependência |
|---|---|---|
| Custo do atestado e das reservas | Com `require_attested` na fila, cada pedido de trabalho refaz o conjunto de atestados (O(nós)) e o ranking filtra os candidatos por unidade (O(unidades × nós)) sob o lock; falta um teto de unidades que exigem atestado. Cada `reserve`/`release` grava o snapshot sob o lock, como o `enqueue`. | código |
| Níveis de confiança | Fluxo de enrollment (token de uso único por nó, aprovação por regra) sobre os níveis já decididos no [[adr-020-niveis-de-confianca-do-no]]. | decisão, código |
| Scheduler local por GPU/NPU | Filas por recurso de GPU e NPU. Hoje só CPU. | hardware: GPU/NPU |
| Localidade | Locality não entra no custo do placement. Depende de saídas residentes no worker: hoje as saídas ficam no controller, então não há transferência a evitar e um desconto por afinidade seria um parâmetro sem efeito medível. | código, depende de F12 (estado e saídas nos workers) |

## F05 — Observabilidade `[x]`

Sem pendências: o código da fase está feito.

## F06 — Estado e recuperação `[x]`

| Item | O que falta | Dependência |
|---|---|---|
| Estado por chave | KV cache, estado de execução e estado de modelo não existem no cluster. Sem isso, não há o que promover além da fila. | código (depende de F12) |
| Replicação da fila | A cópia fica só num nó e o controller a oferece inteira a cada gravação. Falta fator de replicação, portador designado e identidade de cluster na cópia. | código |
| Journal | O snapshot reescreve o estado vivo inteiro a cada mutação. Falta um journal de deltas ou snapshot periódico. | código |
| Checkpoint de unidade | Retomar uma unidade no meio do cálculo. Hoje recalcula do zero. | código |
| Diretório de estado | Referências lógicas de estado (`state://`). | código, decisão |

## F07 — Segurança base `[x]`

| Item | O que falta | Dependência |
|---|---|---|
| Nó desconhecido | Um `node_id` que o registro não conhece passa pela checagem de credencial (as chamadas só respondem "desconhecido"; a cópia do estado não sai para ele). | código |
| Escopo do operador | O operador age sobre qualquer nó, sem escopo. | decisão, código |

## F08 — Inferência `[~]`

| Item | O que falta | Dependência |
|---|---|---|
| Latência do canal de fatias | Cada camada espera a anterior (cerca de 120 idas e voltas por token) e workers no mesmo host disputam núcleos. Medir em máquinas separadas antes de otimizar. | código, hardware: duas máquinas físicas |
| Amostragem | Temperatura, top-p e `n` não existem. | código |
| Stream | Não há `usage` no stream, e `completion_tokens` não conta o token de fim. | código |
| Modelo no gateway | Sem o model runtime distribuído, o gateway concentra o modelo na RAM. | código (depende de F12) |
| Escolha de nó pelo token | O token de inferência escolhe nó, subdiretório, tensor e faixa. | código |
| Tolerância a falha | Só reatribuição. Sem réplica de fatias pré-carregada, o nó que herda uma faixa lê do disco na primeira vez. Sem rebalanceamento quando um nó entra. | código |
| Quarentena e drain | Não derrubam a sessão de inferência; só `revoke` e `leave` derrubam. | código |
| Limite de operações | Com muitos nós e poucos vivos, as faixas movidas passam de 64 operações em voo por nó (503). | código |
| Engine separado do scheduler | Separação completa: o canal de fatias ainda passa pelo controller. | código |

## F09 — Transporte `[~]`

| Item | O que falta | Dependência |
|---|---|---|
| Cluster sobre o `Transport` | O cluster ainda chama o gRPC direto. TCP e socket Unix existem como `Transport`, fora do caminho do cluster. | código |
| E3, memória compartilhada | A variante existe, sem implementação. | código |
| E4, USB | Vai para F18. | hardware: celulares |
| E5, perda e congestionamento | Perda por conexão e congestionamento contínuo (`TCP_INFO` ou netlink). Hoje só há a retransmissão TCP do host, medida na sonda do registro (F02). | código |

## F10 — Inteligência de storage `[~]`

| Item | O que falta | Dependência |
|---|---|---|
| Leitura | Medir leitura sem o cache de página. | código |
| K3, saúde SMART e NVMe | Não existe. | hardware: disco real com SMART |
| K4, alocação dinâmica | Swap, cache de modelo, cache de tensor, checkpoint e reserva em disco. Nada coloca estado em disco por classe. | código |
| K5, adaptação de política | Reduzir escrita quando o desgaste piora. Depende de SMART. | código, hardware: disco real |
| Políticas por classe | HDD e NVMe recebem políticas distintas e mensuráveis. | hardware: HDD e NVMe reais |
| Dashboard de storage | Painel mostra saúde do storage. | código |
| SSD/NVMe como cache ou swap | Não implementado. | código, hardware: NVMe |

## F11 — Request Manager `[~]`

| Item | O que falta | Dependência |
|---|---|---|
| Estados da requisição | Não existem. | código |
| Idempotência | Não existe. | código |
| Equidade e backpressure | Equidade entre tenants e backpressure do spec. Hoje há tetos fixos de admissão. | código |
| Cancelamento no worker | Unidades em execução não podem ser canceladas, e `ShardMatmul` em voo não recebe sinal. | código |
| Deadlines por requisição | Hoje são fixos: 30 s no gateway e 8 s no `ShardMatmul`. | código |
| Orçamento de recursos | Não existe. | código |
| Entrada arbitrária no grafo | Hoje só o grafo sintético e os modelos fixos. | código |
| TLS na perna HTTP | Gateway sem TLS. Fora de loopback só com `AI_CLUSTER_INSECURE`. | código |
| Request Manager separado do scheduler | Ainda no gateway. | código, decisão |

## F12 — Model runtime distribuído `[~]`

| Item | O que falta | Dependência |
|---|---|---|
| N1, registro de modelos | Não há registro central. Modelos são diretórios apontados por variável. | código |
| N2, manifest | Sem lista de tensores nem assinatura. | código, decisão |
| N3, variantes e precisão | Só fp32. Não há quantizados. | código |
| Compatibilidade de arquiteturas | O motor Llama recusa na carga: embeddings não amarrados (a maioria dos 7B tem a saída separada), `rope_scaling` (Llama 3.x), viés de atenção e de MLP, MoE, sliding window e ativações diferentes de `silu`. Só GPT-2 e Llama com embeddings amarrados rodam. Falta suportar saída não amarrada e `rope_scaling`, com paridade numérica contra `transformers`, e publicar no guia a lista de modelos suportados e recusados. | código, dados (pesos de um modelo-alvo para a paridade) |
| Admissão por capacidade | A checagem de memória existe (o gateway recusa na subida um modelo que os nós não comportam). Falta estimar o tempo por token e avisar quando passar de um limite; reservar espaço para o failover; reavaliar quando um worker reinicia com cache menor. | código |
| Memória do gateway com modelo grande | As projeções vão para os workers, mas a tabela de embeddings, as normalizações e o KV cache ficam no gateway. Medir o consumo com um modelo de 7B e decidir se a tabela de embeddings também precisa ser fatiada. | código, dados |
| N4, fatias pré-carregadas | Fatias não são pré-carregadas, porque não há réplica. | código |
| N5, cache de modelo | Não há cache de modelo no controller nem prefetch. | código |
| N7, pipeline | Paralelismo de pipeline não existe. | código |
| N8, tensor | Só row-split de projeções, com entrada replicada e sem all-reduce. | código |
| N9, planejador híbrido | Não existe. | código |
| Placement pelo controller | Hoje o gateway escolhe os nós por variável; o controller só confere o roster. | código |
| Execução fora do gateway | O modelo roda no processo do gateway (cerca de 1,6 GB de RAM). | código |
| Estado KV distribuído | Vive no gateway, por geração, sem persistência. | código |
| Robustez | Sem réplica e sem rebalanceamento. A execução multi-nó funciona, mas não é robusta. | código |

## F13 — Graph engine `[ ]`

| Item | O que falta | Dependência |
|---|---|---|
| L1 a L4 | Modelo de dados de grafo, projeção, travessia e benchmark contra baseline. Não iniciados de propósito. | dados |
| Critério de saída | Só adotar travessia com ganho medido. Falta corpus de conhecimento e uma tarefa de recuperação para comparar. | dados |

## F14 — Edge optimizer `[~]`

| Item | O que falta | Dependência |
|---|---|---|
| M2 a M10 | Cache semântico, recuperação por grafo, seleção de contexto, deduplicação, compressão, resolução de skill e tool, compilador de prompt, orçamento de tokens e caminho rápido. Não iniciados. | código (M3 depende de F13) |
| Medição de ganho | Redução de tokens ou latência acima do custo do otimizador, para cada etapa. Só o M1 (cache exato) foi medido. | código, dados |

## F15 — AI Cluster OS e provisionamento `[~]`

| Item | O que falta | Dependência |
|---|---|---|
| Console do monitor validado | O `aic-console` (tty1, estilo ESXi) e o `aic-role` só foram testados num Alpine e no registro do nó. Falta ver a tela no monitor HDMI do Orange Pi e o `F2`/`F12` num teclado real. | hardware: Orange Pi com monitor e teclado |
| Imagem fora da release | A release do GitHub traz só binários; a imagem do SO embute a chave pública de quem a assina, então cada operador monta a sua. Falta um fluxo documentado para gerar a imagem com a própria chave sem o repositório do autor. | código, documentação |
| J1, imagem de disco para x86 | A imagem existe e foi validada em QEMU/UEFI. Falta validar em hardware real (firmware de placas de rede e vídeo, watchdog da placa-mãe, redimensionamento do ext4 de dados pelo `install-disk.sh` num disco real), Secure Boot e recuperação por rede/PXE. | hardware: PC x86 UEFI |
| J2, imagem de SO | Não existe. O worker só roda como serviço de contêiner ou via `bootstrap.sh`. | código |
| J3, bootstrap em VM | Testar `bootstrap.sh` numa VM com systemd. Hoje só leitura da unit em contêiner. | hardware: VM ou máquina com systemd |
| J4, reimage automático | Reprovisionamento pelo controller, sem operação manual. | código, hardware: máquina limpa |
| J5, A/B em outras placas | Imagem A/B só para armv7 com u-boot. Teste em um nó só. | hardware: mais placas armv7 |
| Assinatura de boot | O u-boot não verifica a imagem. | hardware: placa armv7, TPM ou secure boot |
| J6, ARM64 | Sem imagem nem instalador. | hardware: ARM64 |
| PXE e iPXE | Boot de rede para provisionamento, fora da recuperação. | código, hardware: rede de provisionamento |
| Base Linux específica | Proposta, sem marca de implementação. | decisão |
| Mesmo AI Cluster OS lógico com hardware distinto | Um sistema lógico, com imagens por arquitetura. | hardware: máquinas distintas |
| PCs, servidores, SBCs e celulares | Suporte a cada tipo de máquina. | hardware |
| Reimage após ingresso | Sem reimage automático pelo controller. Hoje é manual. | código, hardware |
| Critério de saída | Máquina limpa vira worker READY sem operação manual. | hardware: máquina limpa |

## F16 — Aceleradores `[ ]`

| Item | O que falta | Dependência |
|---|---|---|
| CPU ARM64 | Worker em 64 bits e paridade numérica. Hoje só armv7. | hardware: ARM64 |
| GPU NVIDIA | Não existe. | hardware: GPU NVIDIA |
| GPU AMD ou Intel | Não existe. | hardware: GPU AMD ou Intel |
| Abstração de NPU | Não existe. | código, hardware: NPU |
| NPU móvel | Não existe. | hardware: celulares com NPU |
| Descoberta de aceleradores | O campo existe no protocolo, mas o worker não o preenche. Precisões fixas em fp32. | código, hardware |
| Scores por workload (M6) | Cluster x86 e ARM com scores por workload. Falta ARM64 e scores por workload. | hardware: x86 e ARM64 |

## F17 — Runtime adaptativo `[~]`

| Item | O que falta | Dependência |
|---|---|---|
| P1, modelo aprendido | A razão real/previsto não persiste e não depende do tamanho da unidade. | código |
| P2, placement preditivo | O placement é função pura dos scores atuais. | código |
| P3, prefetch | Não existe. | código |
| P4, execução especulativa | Não existe. | código |
| P5, inferência de capacidade | Não existe. | código |
| P6, rebalanceamento automático | Reatribuição de faixa é reativa, só em falha. | código |
| Critério de saída | A previsão melhora de forma estatística em workload repetido. | código, dados |

## F18 — Worker móvel `[~]`

| Item | O que falta | Dependência |
|---|---|---|
| Q1, matriz de hardware | Não existe. | hardware: celulares |
| Q2, bootloader e reimage | Não existe. | hardware: celulares |
| Q3, imagem ARM64 | Não existe. | hardware: celulares com ARM64 |
| Q4, transporte USB | Não existe. Só a variante do enum. | código, hardware: celulares |
| Q5, backend de acelerador móvel | Não existe. | hardware: celulares com NPU |
| Q6, bateria | Validada só com sensor simulado. | hardware: celular com bateria real |
| Critério de saída (M7) | Celular reimaged, conectado por USB e contribuindo com compute mensurável. | hardware: celulares |

## F19 — Hardening de produção `[~]`

| Item | O que falta | Dependência |
|---|---|---|
| Lista de revogação | Não há. Uma chave de release ou um certificado vazado vale até a imagem ou o reinício que o retira chegar a cada nó. | código |
| mTLS ligado ao `node_id` | O certificado de cliente prova que o cliente pertence à CA, não qual nó é. | código |
| Rotação e revogação de certificados | Não há rotação, lista de revogação (CRL) nem emissão automática. Os PEM são trocados à mão. | código |
| HTTPS no download do rollout | O download da imagem é só HTTP, e segue redirecionamentos. A URL é conferida só pelo host. | código, hardware: placa armv7 para validar |
| Ondas de rollout | `AI_CLUSTER_ROLLOUT_WAVE` tem teste unitário. Falta validar com mais de um nó atualizável. | hardware: dois nós atualizáveis |
| dm-verity com root hash assinado | Integridade do disco no boot. A medida atual é de software e não prova integridade contra um nó comprometido. | hardware: placa com dm-verity |
| TPM e secure boot | Attestation forte e verificação de boot. | hardware: TPM, placa com secure boot |
| Token de recuperação de uso único (ADR-017) | O token de recuperação já tem escopo mínimo, mas não expira nem é de uso único; trocá-lo exige regravar o `boot.scr`. | código, hardware: placa armv7 para validar |
| Downgrade | O rollout não faz downgrade, e a ordem de versões é numérica. Falta decidir se um rollback legítimo precisa de versão maior. | decisão |
| Marca de versão tentada | Guarda só uma versão. Versões diferentes repetem o ciclo. | código |
| Trilha de auditoria persistente | O log de eventos persiste (`AI_CLUSTER_EVENTS` ou `AI_CLUSTER_DATA_DIR`), mas não é assinado nem à prova de adulteração. | código, decisão |
| Revogação de operador | Não há. O token de operador só troca reiniciando o controller. | código |
| Rotação periódica de tokens | Não há. | código |
| Injeção de falha | Falta partição de rede e controller reiniciado no meio da carga. | código |
| Soak longo | Rodada de horas com medição de recurso. O soak atual mede correção, não memória. | código |
| TLS na perna HTTP do gateway | Pendente. Também listado em F11. | código |
| Exportador de métricas | Escuta em HTTP puro, com o token de operador como bearer. Falta TLS ou mTLS. | código |
| Tempo de boot da medida | A medida lê 512 MiB no boot, de 25 a 40 s num cartão SD, antes de o worker subir. | código |
| Saúde do upgrade | O gate do upgrade local considera saudável só o nó que se registra; um binário que registra e quebra depois passa. Não há rollback sem systemd. | código |

## Marcos (M0 a M7)

| Marco | Estado | O que falta | Dependência |
|---|---|---|---|
| M0, prova de compute em dois nós | Parcial | Duas máquinas físicas. | hardware: duas máquinas físicas |
| M2, provisionamento automático | Não iniciado | Máquina limpa com bootstrap, imagem instalada automaticamente e nó que reingressa READY. | hardware: máquina limpa |
| M3, inteligência de storage | Parcial | Políticas aplicadas dinamicamente, dashboard de saúde, SMART e alocação por classe. | hardware: disco real |
| M4, protótipo LLM em CPU | Parcial | Overhead de comunicação decomposto no trace, e tokens esperado e real. Modelo dividido só nas projeções. | código |
| M5, otimização de borda | Não iniciado | Benchmarks de redução de tokens, cache hit, recuperação por grafo e overhead. Depende de F11, F12 e F13. | código, dados |
| M6, protótipo heterogêneo | Não iniciado | Cluster x86 e ARM, capacidades distintas e scheduler com scores por workload. | hardware: x86 e ARM64 |
| M7, worker móvel | Não iniciado | Celular reimaged, USB e compute estável. | hardware: celulares |
