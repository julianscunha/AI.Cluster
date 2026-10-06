# Plano de Execução

## Estratégia

A implementação será incremental. Cada etapa deve produzir código executável, testes e documentação correspondente.

Não vamos iniciar pelo LLM completo. Primeiro será validada a infraestrutura de computação distribuída.

## Fase A — Fundação do cluster

Objetivo: Controller + Worker + protocolo + CLI.

Entregáveis:

- workspace Rust;
- Protobuf;
- registro de node;
- heartbeat;
- estado de node;
- descoberta básica de hardware;
- CLI inicial.

Critério de saída: um Controller consegue acompanhar pelo menos dois Workers.

## Fase B — Capacidade e telemetria

Objetivo: medir o hardware real.

Entregáveis:

- benchmark de CPU;
- benchmark de memória;
- benchmark de armazenamento;
- benchmark de rede;
- baseline térmico;
- health score;
- capacidade atual;
- coleta de métricas.

Critério de saída: cada node possui um perfil de capacidade e saúde.

## Fase C — Work Unit e transporte

Objetivo: executar trabalho distribuído simples.

Entregáveis:

- Work Unit;
- tensor/buffer;
- transporte TCP;
- chunking;
- checksum;
- métricas de transferência;
- benchmark distribuído.

Critério de saída: uma operação matricial é executada em múltiplos nodes e o tempo é comparado ao baseline local.

## Fase D — Execution Graph e Scheduler

Objetivo: planejar e executar DAGs distribuídos.

Entregáveis:

- [x] Execution Graph;
- [x] dependências;
- [x] placement;
- [x] scheduler global inicial;
- [ ] scheduler local;
- [ ] rebalancing;
- [ ] affinity;
- [ ] deadlines.

Estado atual: o grafo possui Work Units, dependências, validação de ciclos e níveis topológicos. O Placement Planner gera uma atribuição inicial usando o ranking do Scheduler global. O Controller já executa múltiplas Work Units por nível, com concorrência dentro do nível e bloqueio do avanço após falha.

## Fase E — State Manager

Objetivo: tolerância a falhas e hot-plug operacional.

Entregáveis:

- state directory;
- leases;
- checkpoints;
- recuperação;
- promoção de réplicas;
- drain.

## Fase F — Model Manager

Objetivo: modelos versionados e particionáveis.

Entregáveis:

- registry;
- variantes;
- shards;
- model cache;
- prefetch;
- compatibilidade.

## Fase G — Edge Intelligence

Objetivo: reduzir trabalho antes da inferência.

Entregáveis:

- fast path;
- exact/semantic cache;
- Graph retrieval;
- deduplicação;
- redução de contexto;
- tool/skill resolution;
- prompt compiler;
- token budget.

## Fase H — Inference Engine

Objetivo: executar um Transformer real.

Entregáveis:

- prefill;
- decode;
- KV cache;
- pipeline parallelism;
- tensor parallelism;
- modo híbrido;
- speculative decoding.

## Fase I — AI Cluster OS

Objetivo: transformar nodes aprovados em appliances Linux dedicados.

Entregáveis:

- imagem imutável;
- provisioning;
- reimagem;
- boot/recovery;
- hardware backends;
- storage intelligence.

## Fase J — API e operação

Objetivo: disponibilizar o cluster a consumidores externos.

Entregáveis:

- Inference API;
- streaming;
- autenticação;
- limites;
- cancelamento;
- métricas por requisição;
- TUI operacional.

## Regra de evolução

Nenhuma hipótese de desempenho deve virar requisito definitivo sem benchmark.

Resultados experimentais devem permanecer identificados como experimentais até validação.
