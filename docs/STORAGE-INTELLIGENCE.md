# Inteligência de Armazenamento

## Objetivo

Transformar o armazenamento local em um recurso gerenciado de cache, staging, estado e swap, de acordo com tipo, desempenho e saúde.

## Detecção

O AI Cluster OS identifica:

- HDD;
- SSD SATA;
- SSD NVMe;
- capacidade;
- latência;
- throughput;
- temperatura;
- saúde;
- desgaste/endurance.

## Política

HDD recebe uso conservador de swap e cache.

SSD SATA pode receber cache e staging ampliados.

NVMe saudável e rápido pode receber uso agressivo de cache, staging, checkpoints e swap, sempre mantendo margem operacional.

## Particionamento lógico

A capacidade útil é dividida entre:

```
OS / recovery
swap
model cache
tensor cache
state / checkpoint
reserva
```

As proporções são dinâmicas.

## Swap

Swap é extensão de memória virtual, não equivalente a RAM. Deve atuar principalmente como mecanismo de pressão de memória e não como estratégia primária de desempenho.

## Cache

Cache deve ser priorizado quando evitar transferência, recomputação ou carregamento repetido.

## Saúde

A política deve reduzir escrita e uso intensivo quando o armazenamento apresentar desgaste, temperatura ou erros anormais.

## Telemetria

Registrar throughput, latência, IOPS, temperatura, saúde, desgaste e pressão de espaço.

## Reclassificação

O perfil de armazenamento pode mudar dinamicamente. O sistema deve recalcular a política quando detectar degradação.

## Critério

Um node com CPU modesta e NVMe excelente pode ter alto valor como cache, staging ou checkpoint worker.
