---
title: "Runtime do worker"
aliases: ["Runtime do worker", "Worker Runtime", "docs/07", "07 — Worker Runtime", "07 — Runtime do worker"]
tags: [sistema/subsistema/substrato]
type: sistema/subsistema
status: decidido
implementacao: parcial
fases: ["[[fase-03-execucao-distribuida]]", "[[fase-08-inferencia]]"]
---

# Runtime do worker

Executa Work Units usando o hardware local mais adequado. É o processo que roda em cada máquina do cluster e fala com o controller pelo [[camada-de-transporte]].

## Responsabilidade

- receber Work Units do controller e executá-las;
- medir e reportar capacidade e saúde ([[capability-engine]], [[saude-e-confianca]]);
- manter cópia de estado do controller, quando configurado;
- manter o canal residente de fatias de peso, quando configurado (ver abaixo).

## Componentes

| Componente | Papel |
|---|---|
| Work Unit Engine | Executa Work Units e devolve o resultado com checksum. |
| Memory Manager | Limita memória por execução. |
| Tensor Engine | Kernels de álgebra linear por backend. |
| Device Manager | Descreve e aciona os dispositivos locais. |
| Local Scheduler | Decide núcleos, GPU/NPU e buffers do nó. |
| Transport Engine | Envia e recebe dados pelos transportes disponíveis. |
| Cache Manager | Guarda dados reutilizáveis (slices de peso, resultados). |
| Checkpoint Manager | Persiste progresso para recuperação. |
| Telemetry | Eventos, métricas e traces do nó. |

## Backends de hardware

Interface abstrata do Tensor Engine:

```text
Tensor Engine
 |- CPU backend
 |- CUDA/compatible GPU backend
 |- ROCm/compatible backend
 |- Vulkan/OpenCL candidate
 `- NPU backend
```

Começa com CPU x86_64 e ARM64.

## Escalonamento local

O Global Scheduler decide o nó. O Local Scheduler decide núcleos de CPU, GPU/NPU e buffers dentro do nó. ([[scheduler]], [[f04-scheduler-local]])

## Prioridade

Classes: `CRITICAL`, `INTERACTIVE`, `NORMAL`, `BACKGROUND`, `PREFETCH`, `CACHE`. A ordem decide qual trabalho local sai da fila primeiro.

## Compute vs transferência

O runtime pode decidir entre transferir um tensor e recomputá-lo, quando a previsão indicar menor custo total. A decisão usa o [[modelo-de-desempenho]] e os links do [[camada-de-transporte]].

## Zero-copy

EXPERIMENTAL: zero-copy e DMA quando hardware e driver suportarem.

> [!info] O que já existe
> **Ciclo do worker** (`code/crates/worker/src/main.rs`):
> - Loop de trabalho: pede unidades ao controller (`RequestWork`); sem trabalho, espera `WORK_POLL_IDLE_MS` (200 ms); após concluir uma, pede de novo sem esperar. Devolve o resultado com `SubmitWorkResult`.
> - Execução: `execute_work` valida o tamanho das matrizes, multiplica com os núcleos concedidos (`min(cpu_threads reportadas, disponíveis)`) e devolve checksum e tempo de cálculo.
> - Heartbeat em tarefa própria, a cada 5 s, para que uma unidade longa não faça o controller tirar o nó do ar.
> - Saída planejada: SIGTERM ou Ctrl+C param de pegar trabalho e avisam o controller antes de sair.
> - Identidade: `node_id` e credencial por nó, persistida em arquivo (`AI_CLUSTER_NODE_CREDENTIAL_FILE`).
> - Cópia de estado (I4): com `AI_CLUSTER_STATE_REPLICA`, guarda a cópia do estado do controller que o controller envia no heartbeat quando a sua está atrasada, e a oferece de volta no re-registro.
>
> **Tensor Engine**: só CPU. O kernel de matmul em `f64` está em `code/crates/common/src/matrix.rs` (`multiply_with_threads`). O modelo de linguagem (`code/crates/engine`) roda no gateway; o worker só executa fatias.
>
> **Canal residente de fatias (J5)**: `code/crates/worker/src/shards.rs` e `ShardStore` em `code/crates/engine/src/shard.rs`.
> - Ativo só com `AI_CLUSTER_MODEL_DIR`. O cache de fatias tem teto em `AI_CLUSTER_SHARD_CACHE_MB` (padrão 2048 MiB) com despejo LRU.
> - O worker abre o stream `ShardChannel` com o `node_id` no cabeçalho e reconecta a cada 2 s após queda ou recusa.
> - Cada frame `ShardCompute` é respondido com `ShardResult`. Até 2 cálculos em paralelo (`CONCURRENT_COMPUTES`), fila de respostas de 64.
> - Só tensores de projeção de camada (11 sufixos reconhecidos por `is_projection`) são lidos do disco. Caminho fora da raiz do modelo é recusado. Fatia de até 256 MiB; saída de até 64 MiB.
> - Erro de pedido (tamanho incoerente, tensor não permitido) volta com `invalid = true`: o controller não tenta outro nó por causa dele.
> - Pesos não cruzam a rede. Só entram ativações e saídas.
>
> Fases: [[f08-canal-residente-de-fatias]], [[f08-engine-local]], [[f03-grafo-de-dependencias]].

> [!todo] Backends GPU, ROCm, Vulkan/OpenCL e NPU; Memory Manager com teto por execução; Local Scheduler com prioridades e buffers (hoje o controller entrega uma unidade por vez e o worker usa todos os núcleos concedidos); Cache Manager para resultados; Checkpoint Manager do worker (o checkpoint existe só no controller); Telemetria própria do nó; zero-copy e DMA; decisão de recompute. Ver [[fase-17-runtime-adaptativo]] e [[fase-16-aceleradores]].

> [!warning] Divergência: o Local Scheduler do alvo decide núcleo, GPU/NPU e buffer. Hoje a única decisão local é quantos núcleos usar. A cópia de estado de I4 é um mecanismo do controller, guardado no worker, e não o Checkpoint Manager descrito no alvo.
