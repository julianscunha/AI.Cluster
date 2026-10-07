# Modelo de Dados

## Entidades principais

```
Cluster
Node
Device
Capability
Health
Transport
Model
ModelVariant
ModelShard
Request
Execution
ExecutionPlan
ExecutionGraph
PlacementPlan
WorkUnit
DataEdge
Tensor
StateObject
Metric
Event
Trace
GraphEntity
GraphRelation
```

## Node

Identidade lógica do worker dentro do cluster.

Campos principais:

- node_id;
- device_id;
- estado;
- trust_state;
- versão do OS;
- versão do runtime;
- timestamps.

## Device

Representa o hardware físico: arquitetura, CPU, memória, aceleradores, armazenamento e interfaces de rede.

## Capability

Representa capacidade medida e atualizada por telemetria.

Exemplos:

```
general_compute
matmul_int4
matmul_int8
fp16
memory_bandwidth
network
storage
```

## Health

Representa saúde por domínio:

```
CPU
memória
armazenamento
rede
térmico
aceleradores
```

## Transport

Representa enlaces entre recursos e suas propriedades de largura de banda, latência, perda, tipo e saúde.

## Model / Variant / Shard

Separam identidade do modelo, representação de precisão e fragmentação física.

## Request / Execution / Plan

- Request: intenção externa.
- Execution: execução concreta daquela requisição.
- Plan: estratégia de execução e placement.

## WorkUnit

Unidade executável. Possui payload, operação, prioridade e requisitos de execução. Dependências de controle e referências de dados pertencem ao Execution Graph.

## DataEdge

Relação explícita de produção/consumo entre Work Units. Na implementação inicial, o DataEdge referencia a Work Unit de origem e o Controller transporta seu output como bytes para o payload da consumidora.

A representação futura deverá incluir tipo de buffer, shape, dtype, tamanho, checksum, localização e estratégia de transporte.

## Tensor

Objeto de dados endereçável, com shape, dtype, tamanho, checksum e localização.

## StateObject

Objeto de estado como KV cache, checkpoint e estado de execução.

## Observabilidade

Metrics são séries temporais; Events registram mudanças; Traces registram o percurso de uma execução.

## Grafo

O grafo é uma projeção das entidades e relações operacionais. A perda do grafo deve ser recuperável a partir dos dados primários.

## Regra de consistência

Identidade e estado de controle têm consistência forte. Caches descartáveis podem usar políticas mais flexíveis.

Grandes buffers e artefatos de modelo não devem ser armazenados como blobs em estruturas relacionais ou de grafo.
