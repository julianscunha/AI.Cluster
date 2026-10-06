# Grafo de Execução

## Objetivo

Representar o trabalho como um grafo executável de operações e dependências.

## Separação

```
Model Graph
   ↓
Execution Graph
   ↓
Placement Plan
   ↓
Work Units
```

O Execution Graph responde **o que** precisa ser executado. O Placement Plan responde **onde**.

## Vértices

Cada vértice representa uma operação ou etapa executável.

## Arestas

Arestas representam:

- dependência de dados;
- ordem;
- sincronização;
- produção/consumo de tensor.

## Paralelismo

Operações independentes podem executar em paralelo:

```
       W1 ──┐
       W2 ──┼──→ SYNC → W4
       W3 ──┘
```

## Subgrafos

O Planner pode particionar o grafo em subgrafos conforme hardware e topologia.

## Replanejamento

Quando um node entra, sai ou se degrada, o Execution Graph lógico pode permanecer; o Placement Plan recebe nova versão.

## Épocas

Execuções autoregressivas podem usar epochs para pontos seguros de replanejamento.

```
Epoch 70 → Plan 281
Epoch 71 → Plan 281
Epoch 72 → Plan 282
```

## Speculative Execution

Uma Work Unit identificada como straggler pode ser executada em paralelo em outro node, desde que a política permita.

## Uso fora de LLM

O mesmo mecanismo deve suportar processamento de documentos, áudio, visão, embeddings, reranking e outros workloads.

## Critério de projeto

O Execution Graph deve ser independente do hardware físico e o Placement Plan deve ser independente do histórico bruto de telemetria.
