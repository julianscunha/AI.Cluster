# Agendador

## Objetivo

Escolher onde e quando cada Work Unit será executada, considerando capacidade, localidade de dados, modelo, rede, saúde, temperatura, confiança, fila, prazo e custo de migração.

## Arquitetura

Existem dois níveis:

- Agendador global: decide node, dispositivo e placement.
- Agendador local: distribui trabalho entre CPU, GPU, NPU e memória do node.

## Fluxo de decisão

```
Work Unit
→ filtrar incompatíveis
→ avaliar capacidade
→ avaliar localidade
→ avaliar rede
→ avaliar fila
→ avaliar saúde
→ avaliar risco térmico
→ calcular custo
→ escolher placement
```

## Pontuação

A função de custo deve ser configurável para priorizar latência, throughput, confiabilidade, eficiência ou equilíbrio.

## Localidade

Dados, shards de modelo e estado já presentes no node recebem preferência quando o ganho de comunicação superar a diferença de capacidade.

## Prazo

Work Units interativas podem ter deadline. O agendador elimina candidatos cuja estimativa exceda o prazo.

## Rebalanceamento

Entrada, saída ou degradação de node pode gerar novo plano. O sistema deve usar limiar de melhoria para evitar movimentação excessiva.

## Histerese

Pequenas melhorias não justificam migração. O sistema deve comparar benefício esperado e custo de replanejamento.

## Stragglers

Work Units significativamente mais lentas que a previsão podem receber execução especulativa quando o custo adicional for justificável.

## Saúde térmica

A capacidade efetiva de um node pode ser reduzida por throttling, temperatura, pressão de memória ou outros sinais de degradação.

## Segurança

Nodes não confiáveis ou em quarentena não participam de workloads protegidos.

## Critério

O Scheduler deve justificar sua decisão com fatores, custos estimados e confiança da previsão.
