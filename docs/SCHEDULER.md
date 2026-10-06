# Agendador

## Objetivo

Escolher onde e quando cada Work Unit será executada, considerando capacidade, localidade de dados, modelo, rede, saúde, temperatura, confiança, fila, prazo e custo de migração.

## Arquitetura

Existem dois níveis:

- Agendador global: decide node, dispositivo e placement.
- Agendador local: distribui trabalho entre CPU, GPU, NPU e memória do node.

A seleção global opera sobre o estado em memória do Controller. O Scheduler não consulta o banco de dados para cada Work Unit.

## Seleção atual

A implementação inicial calcula um score por Worker elegível usando quatro fatores:

| Fator | Peso |
|---|---:|
| Capacidade de compute | 50% |
| Saúde | 25% |
| Carga ativa | 15% |
| RTT de rede | 10% |

A capacidade de compute usa `cpu_ops_per_sec` quando disponível e `cpu_logical_cores` como fallback. A carga ativa é medida pelo número de Work Units atualmente reservadas pelo Controller.

A fórmula atual é:

```text
score =
  0.50 × capacidade
+ 0.25 × saúde
+ 0.15 × carga
+ 0.10 × rede
```

Cada componente é normalizado entre os candidatos quando aplicável. Menor RTT e menor carga aumentam o score.

Essa fórmula é deliberadamente inicial. Os pesos não são requisito definitivo e deverão ser recalibrados com dados de execução real.

## Fluxo de decisão

```text
Work Unit
→ filtrar incompatíveis
→ avaliar capacidade
→ avaliar localidade
→ avaliar rede
→ avaliar carga
→ avaliar saúde
→ avaliar risco térmico
→ calcular score
→ reservar capacidade
→ executar
→ liberar capacidade
```

## Rede

O Worker mede o RTT de uma chamada gRPC de controle entre Worker e Controller. O valor armazenado em `Capability.network_rtt_us` representa RTT do plano de controle, incluindo o overhead do transporte gRPC; não é uma medição ICMP nem uma capacidade de throughput.

A medição usa uma chamada de aquecimento e múltiplas amostras, reportando a mediana.

## Carga

Ao selecionar um Worker, o Controller incrementa `active_work_units` antes de iniciar a execução. Após a conclusão ou falha, a reserva é liberada.

Isso evita usar `last_heartbeat_unix_ms` como proxy de disponibilidade ou carga.

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
