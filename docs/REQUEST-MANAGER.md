# Gerenciador de Requisições

## Objetivo

Controlar o ciclo de vida de cada requisição externa antes e durante sua execução no cluster.

## Responsabilidades

- autenticar e validar a requisição;
- aplicar limites;
- normalizar a entrada;
- criar Request e Execution;
- determinar prioridade;
- calcular orçamento inicial;
- controlar filas e concorrência;
- aceitar cancelamento;
- acompanhar consumo e estado.

## Estados

```
RECEIVED
VALIDATING
OPTIMIZING
QUEUED
PLANNED
RUNNING
CANCELLING
COMPLETED
FAILED
CANCELLED
```

## Orçamento

Registrar tokens de entrada, limite de saída, tokens esperados, capacidade de contexto, latência prevista e recursos estimados.

## Concorrência

Limites são aplicados antes do Scheduler. Filas podem ser separadas por prioridade.

## Cancelamento

O cancelamento externo deve interromper novas Work Units e propagar o sinal para operações canceláveis.

## Isolamento

Cada requisição possui identidade e escopo próprios. Estado, modelos e dados não podem ser acessados por outra requisição sem autorização.

## Telemetria

Cada requisição possui trace, contagem de tokens, latência, otimizações aplicadas, recursos utilizados e motivo de falha quando houver.

## Critério

O consumidor recebe uma resposta estável mesmo quando a execução interna for distribuída ou replanejada.
