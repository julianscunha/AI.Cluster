---
title: "AI Cluster — Inference Engine"
type: spec
status: decided
tags: [spec, inference-engine]
---

# AI Cluster — Inference Engine

Status: DECIDED baseline / EXPERIMENTAL implementation details

## 1. Objetivo

O Inference Engine executa um Model Execution Graph sobre o conjunto de recursos selecionado pelo Execution Planner e pelo Scheduler.

Ele deve suportar, progressivamente:

- prefill;
- decode autoregressivo;
- KV cache;
- tensor parallelism;
- pipeline/model parallelism;
- hybrid parallelism;
- quantização;
- batching e micro-batching;
- overlap compute/communication;
- speculative decoding;
- checkpoint/recovery.

## 2. Separação de responsabilidades

```text
Request Manager
  -> ciclo de vida da request

Execution Planner
  -> estrutura e placement esperado

Scheduler
  -> alocação de recursos

Inference Engine
  -> execução matemática do modelo
```

O Inference Engine não decide a topologia global do cluster.

## 3. Fases de inferência

Para um modelo autoregressivo:

```text
Request
  -> Tokenization
  -> Prefill
  -> KV Cache creation
  -> Decode loop
  -> Output
```

### Prefill

Processa o contexto de entrada e cria o estado necessário para geração.

### Decode

Gera tokens sequencialmente, reutilizando o KV cache.

```text
Token N
  -> model
  -> Token N+1
  -> model
  -> Token N+2
```

## 4. Execution Graph do Transformer

O runtime deve representar pelo menos:

```text
Embedding
  -> Transformer Block x N
      -> Norm
      -> Attention
      -> Residual
      -> Norm
      -> FFN
      -> Residual
  -> Final Norm
  -> LM Head
```

Os nós internos podem ser agrupados em subgrafos otimizados.

## 5. Kernel abstraction

A execução de cada operação passa pela Hardware Abstraction Layer.

```text
Inference Engine
  -> Tensor API
  -> Hardware Backend
       -> CPU backend
       -> GPU backend
       -> NPU backend
```

O Engine não deve codificar instruções específicas de hardware diretamente no grafo do modelo.

## 6. Tensor representation

Cada tensor deve carregar:

```text
id
shape
strides
dtype
precision
layout
device
memory residency
version
checksum, quando necessário
```

Layouts e formatos devem ser tratados pelo runtime para evitar conversões redundantes.

## 7. Memory planning

Antes da execução, o Engine deve construir um plano de memória previsível quando possível:

```text
persistent weights
activation buffers
KV cache
workspace
communication buffers
```

Buffers temporários devem ser reutilizados por lifetime analysis em vez de alocados e liberados repetidamente.

## 8. KV Cache

O KV cache é estado de execução associado à sequência.

```text
Request
  -> KV Cache
      -> layer 1..N
```

Deve suportar:

- paginação/chunking;
- residência HOT/WARM;
- compressão/quantização quando validada;
- replica/checkpoint quando requerido pelo State Manager;
- eviction de estado não crítico.

## 9. Paged KV concept

O KV cache deve ser administrado em blocos lógicos, evitando exigir um grande segmento contíguo de memória.

```text
KV Cache
  -> page 001
  -> page 002
  -> page 003
  -> ...
```

Isso facilita crescimento, movimentação e recuperação.

## 10. Decode scheduler

Cada passo de decode gera um conjunto de operações dependentes.

```text
KV lookup
  -> attention
  -> FFN
  -> logits
  -> sampling
  -> next token
```

O próximo passo somente começa quando o estado necessário estiver disponível.

## 11. Tensor parallelism

Uma operação pode ser particionada entre vários workers.

```text
MatMul
  -> shard A -> node 01
  -> shard B -> node 02
  -> shard C -> node 03
  -> reduce
```

A estratégia deve ser usada quando o ganho de compute exceder o custo de comunicação e sincronização.

## 12. Pipeline parallelism

Blocos do modelo podem ser atribuídos a estágios:

```text
Stage 1 -> Node Group A
Stage 2 -> Node Group B
Stage 3 -> Node Group C
```

As ativações atravessam os estágios.

## 13. Hybrid parallelism

O planner pode combinar ambos:

```text
Stage 1
  -> tensor parallel nodes A/B

Stage 2
  -> node C

Stage 3
  -> tensor parallel nodes D/E/F
```

Essa é a estratégia preferencial para hardware heterogêneo, mas deve ser validada experimentalmente.

## 14. Communication/computation overlap

O Engine deve permitir:

```text
compute current chunk
    ||
transfer next chunk
    ||
prefetch following data
```

Streams/queues independentes podem ser usados quando o backend suportar.

## 15. Prefill versus decode

O Planner deve tratá-los como cargas distintas.

### Prefill

Normalmente possui maior paralelismo sobre o contexto.

### Decode

É mais sensível a:

```text
latency
KV locality
synchronization
memory bandwidth
```

Uma topologia ótima para prefill pode não ser ótima para decode.

## 16. Continuous batching

Quando houver múltiplas requests, o Engine pode agrupar sequências em execução para aumentar utilização do hardware.

```text
request A
request B
request C
    -> batch
```

O primeiro MVP pode operar com uma request por vez, mas a abstração deve permitir batching futuro.

## 17. Single-request optimization

Como o projeto quer acelerar uma única request, o Engine deve priorizar:

```text
tensor parallelism
pipeline parallelism
operation parallelism
prefetch
overlap
speculative decoding
```

Não deve depender exclusivamente de batching para obter performance.

## 18. Speculative decoding

Pode haver um modelo draft menor:

```text
Draft Model
  -> candidate tokens
  -> Main Model verification
```

O cluster pode utilizar hardware fraco para o draft e hardware forte para verificação.

A decisão de usar speculative decoding deve considerar:

```text
acceptance rate
communication cost
draft latency
verification latency
energy/cost
```

## 19. Sampling

O sampling deve ser separado do cálculo do modelo.

Parâmetros possíveis:

```text
temperature
top_k
top_p
seed
repetition policy
```

O estado de sampling faz parte do Execution State.

## 20. Determinismo

O runtime deve oferecer modo determinístico quando suportado:

```text
seed fixed
controlled kernels
stable scheduling
```

Isso facilita testes, checksum de resultados e diagnóstico.

## 21. Quantization

O Engine deve conhecer as variantes suportadas do modelo e suas operações.

```text
weights = Q4
compute = INT8/INT4 where supported
accumulation = wider precision where required
```

Qualquer conversão deve ser explícita no Execution Graph e contabilizada no custo.

## 22. Mixed precision

O planner pode escolher precisões diferentes por operação, desde que o Model Profile e o backend garantam validade.

O objetivo é preservar qualidade onde necessário e reduzir custo onde possível.

## 23. Checkpoint/recovery

Pontos de checkpoint podem ocorrer após estágios seguros, especialmente em operações longas.

```text
Execution Epoch
  -> compute
  -> checkpoint
  -> next epoch
```

O State Manager controla persistência e recuperação. O Inference Engine fornece os hooks necessários.

## 24. Failure semantics

Uma falha deve ser classificada como:

```text
recoverable work failure
node failure
transport failure
state corruption
model incompatibility
numerical failure
```

O Engine não deve ocultar a causa real.

## 25. Numerical validation

Para operações críticas, o runtime pode validar:

```text
shape
NaN/Inf detection
range checks
checksum
kernel output invariants
```

Validação completa não deve ser ativada indiscriminadamente em caminho quente.

## 26. Performance instrumentation

Cada etapa deve poder produzir:

```text
prefill_time
decode_time
kernel_time
memory_time
communication_time
sampling_time
```

O trace deve preservar a relação com Work Unit e Node.

## 27. Hardware-aware execution

O Engine consulta capabilities locais:

```text
supported_dtype
vector_width
accelerator
memory capacity
memory bandwidth
```

E seleciona kernels compatíveis.

## 28. Local optimization

O Worker Runtime pode realizar otimizações locais que não alterem a semântica do plano global:

```text
kernel fusion
buffer reuse
prefetch
local scheduling
cache reuse
```

## 29. Global optimization boundary

Mudanças como:

```text
shard placement
global topology
replication
pipeline boundaries
```

continuam sob responsabilidade do Controller/Planner.

## 30. Admission requirements

Antes de iniciar uma execução, o Engine deve receber um plano que informe pelo menos:

```text
model variant
execution plan
required shards
precision
memory budget
transport requirements
state references
```

## 31. Completion

Uma execução só é considerada concluída após:

```text
final token/state finalized
output committed
state release/retention policy applied
metrics flushed
trace completed
```

## 32. MVP experimental

O primeiro Engine deve ser deliberadamente menor:

```text
CPU-only
x86_64
single request
single model
Q4/Q8
pipeline parallelism
simple tensor transport
basic KV cache
```

Depois:

```text
ARM64
GPU
NPU
tensor parallelism
hybrid planning
speculative decoding
```

## 33. Critério de sucesso

O MVP deve provar que:

```text
modelo > memória de um node
AND
modelo pode ser executado em múltiplos nodes
AND
uma única request pode atravessar os nodes
AND
resultado é correto
AND
performance pode ser medida
```

O benchmark deve comparar também com um baseline mononode para determinar se a distribuição realmente acelera a execução.
