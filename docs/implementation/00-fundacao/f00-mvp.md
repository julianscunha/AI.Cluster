---
title: "F00 — MVP: primeira build e primeiro alvo experimental"
aliases: ["implementation/mvp", "EP §20", "Primeiro alvo experimental", "M0"]
tags: [implementacao/fase/00, implementacao/fatia, implementacao/estado/parcial]
type: implementacao/fatia
status: parcial
superado: false
fase: "[[fase-00-fundacao]]"
codigos-antigos: ["EP §20", "implementation/mvp", "M0"]
sistema: ["[[arquitetura]]", "[[modelo-de-desempenho]]"]
evidencia: ["code/tests/scaling-sweep.sh", "code/tests/phase-2-graph.sh"]
---
# F00 — MVP: primeira build e primeiro alvo experimental

- [ ] Superado

## Objetivo

Provar que hardware heterogêneo pode colaborar na execução de uma única carga distribuída **sem depender de um LLM**. O primeiro experimento não é inferência: é um workload de matriz/DAG em CPU, medido contra o custo de comunicação.

A pergunta experimental que orienta a plataforma inteira:

> Quando a comunicação é adicionada, em quais tamanhos de Work Unit o hardware adicional produz ganho líquido?

## Hardware

- 2–4 máquinas Linux x86_64, CPU-only;
- 4–16 GB de RAM cada, SSD quando disponível;
- Ethernet 1 GbE.

## Software

- Controller e Worker Agent;
- protocolo de Work Unit;
- backend CPU de tensor/matriz;
- telemetria (eventos e métricas);
- CLI.

## Carga de trabalho

Multiplicação de matrizes, operações elementwise e DAG de dependências.

```text
Request
 -> Work Unit A
 -> Work Unit B + Work Unit C in parallel
 -> Work Unit D
 -> response
```

## Medições

Para cada configuração de nós (1, 2, 3 e 4 nós), comparar:

| grandeza | o que mede |
|---|---|
| tempo de compute | execução da unidade no worker |
| tempo de comunicação | transferência e serialização |
| latência total | ponta a ponta do request |
| throughput | unidades por segundo |
| previsto × real | erro do modelo de custo ([[f05-previsto-vs-real]]) |
| memória, CPU, fila | sinais de nó e de fila |
| ganho ou perda por nó adicionado | a grandeza que responde a pergunta |

## Critérios de sucesso

O MVP só é bem-sucedido se demonstrar **os dois lados**: casos em que adicionar um nó melhora tempo ou throughput, e casos em que adicionar um nó piora o resultado por causa da comunicação.

## Resultado do primeiro experimento

Executado: `code/tests/scaling-sweep.sh` (1, 2 e 3 workers × tamanhos de matriz 32–256, ver [[f03-varredura-compute-vs-transferencia]]).

- Dentro do teto de mensagem do gRPC, a transferência ainda empata ou supera o compute em todos os tamanhos medidos.
- O crossover (tamanho em que a distribuição passa a compensar) fica **acima de 256**; para observá-lo é preciso teto de mensagem maior ou outro transporte ([[camada-de-transporte]]).
- A varredura achou um gargalo real: o poll do worker compartilhava o ticker do heartbeat (5 s), fixando a parede em ~36 s para 6 unidades. Corrigido em `3caa75d`.

> [!warning] Teto de medição
> O experimento foi rodado em host único (containers) para a mecânica. A comparação que importa — dispatch entre máquinas custa menos que o compute que economiza — exige as duas máquinas físicas do [[f00-runbook-duas-maquinas]]. Até lá, o ganho líquido entre máquinas não está demonstrado.

## Checklist

- [x] Primeiro experimento executado: 1/2/3 nós × tamanhos 32–256 (`code/tests/scaling-sweep.sh`).
- [x] Gargalo de poll identificado e corrigido.
- [x] Métricas de compute, transferência e total registradas (`[[f05-metricas]]`).
- [ ] Crossover observado acima de 256 (exige teto maior ou transporte novo).
- [ ] Comparação entre máquinas físicas (4 nós, 1 GbE).

## Limites

- Host único: a mecânica é provada, a afirmação de distribuição não.
- Teto de mensagem gRPC limita o tamanho da unidade medida.

## Próximos passos

- Rodar o runbook em duas máquinas físicas e registrar a tabela de execução.
- Subir o teto de mensagem ou testar um transporte alternativo para achar o crossover.
