# Work Unit

## Definição

Work Unit é a menor unidade de trabalho que o Controller consegue medir, agendar, executar, validar, repetir e rastrear.

## Estrutura conceitual

```
WorkUnit
├── identidade
├── tipo de operação
├── requisitos
├── dependências
├── entrada
├── saída
├── prioridade
├── deadline
├── política de retry
└── métricas
```

## Exemplos

- MATMUL;
- ATTENTION;
- NORMALIZATION;
- EMBEDDING;
- COMPRESSION;
- CACHE_LOOKUP;
- GRAPH_QUERY.

## Requisitos

Uma Work Unit pode declarar:

- memória mínima;
- precisão;
- acelerador obrigatório/preferencial;
- latência máxima;
- prioridade;
- necessidade de rede.

## Dependências

Work Units formam um grafo acíclico de execução quando a carga exigir dependências.

## Estados

```
CREATED
READY
QUEUED
RUNNING
CHECKPOINTED
COMPLETED
FAILED
RETRYING
CANCELLED
```

## Idempotência

Sempre que possível, operações devem ser repetíveis com o mesmo input e versão.

## Checksum

Inputs e outputs podem possuir checksum para validação e recuperação.

## Retry

Retry considera tipo da operação, estado, deadline e capabilities do novo worker.

## Speculative Execution

Pode duplicar uma Work Unit lenta quando o ganho esperado superar o custo computacional adicional.

## Observabilidade

Cada Work Unit deve gerar métricas de fila, transporte, computação, memória e tempo total.
