# Observabilidade

## Objetivo

Disponibilizar visão em tempo quase real do cluster, das requisições, dos nodes e das causas de degradação.

## Três tipos

### Métricas

Valores contínuos de hardware, runtime e inferência.

### Eventos

Mudanças discretas como NODE_JOINED, NODE_DEGRADED, NODE_LOST e REBALANCE_STARTED.

### Traces

Percurso detalhado de uma requisição e suas Work Units.

## Métricas de node

CPU, frequência, temperatura, memória, swap, GPU/NPU, armazenamento, rede, throttling e saúde.

## Métricas de inferência

Input tokens, output tokens, tokens evitados, TTFT, ITL, tokens por segundo, latência total, contexto e KV Cache.

## Métricas de execução

Tempo de fila, transferência, descompressão, computação, escrita de cache, checkpoint e tempo total.

## Expected versus actual

O sistema deve mostrar previsão, resultado observado e erro de previsão.

## Baseline

Thresholds dependem do hardware. O Health Engine deve manter baseline por dispositivo e histórico.

## Diagnóstico

Eventos, métricas e traces podem ser correlacionados para indicar a provável causa de uma degradação.

## Amostragem adaptativa

Em condição normal usa-se coleta econômica. Em degradação, a frequência aumenta automaticamente.

## Armazenamento

Dados brutos de alta frequência têm retenção curta; agregados têm retenção maior.

## TUI

A interface deve mostrar:

```
estado
saúde
capacidade
tokens
latência
throughput
rede
storage
temperatura
Work Units
eventos
```

## Detalhe de node

Deve permitir identificar causa de degradação, como throttling térmico, pressão de memória, falha de armazenamento ou degradação de rede.

## Diagnóstico

O sistema deve explicar o impacto estimado de um problema sobre a capacidade do cluster.
