---
title: "F00 — Próximos testes (snapshot da Fase 0)"
aliases: ["phase-0/next-tests", "Próximos testes", "Testes iniciais"]
tags: [implementacao/fase/00, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: true
fase: "[[fase-00-fundacao]]"
codigos-antigos: ["phase-0/next-tests"]
sistema: ["[[camada-de-transporte]]", "[[modelo-de-desempenho]]"]
evidencia: ["code/tests/phase-1-leave.sh", "code/tests/phase-2-graph.sh"]
---
# F00 — Próximos testes (snapshot da Fase 0)

- [x] Superado

> [!warning] Superado por [[fase-00-fundacao]] e [[f03-varredura-compute-vs-transferencia]]
> Esta lista era o plano de testes da Fase 0 e a medição de um único host. O estado atual está no checklist da fase (gate A1–A5) e na varredura de 1/2/3 workers, que corrigiu o gargalo de poll. Fica como registro histórico da mecânica provada em host único.

## Objetivo

Lista de testes da Fase 0 com o estado de cada um: VERIFICADO significa que o teste roda e a saída foi observada; PENDENTE significa que ainda não foi executado. Containers no mesmo host provam a mecânica, não a afirmação de desempenho.

## Testes (estado no snapshot)

1. Registro no controller com um worker. — VERIFICADO (`ai-cluster nodes`).
2. Detecção de perda de heartbeat. — VERIFICADO (estado vira READY → OFFLINE após a grace).
3. Ingestão de capability report. — VERIFICADO (threads/RAM/health reportados pelo worker).
4. Round trip TCP entre dois nós. — VERIFICADO com dois workers em um host; máquinas separadas ainda PENDENTE.
5. Baseline de benchmark de matriz de nó único. — VERIFICADO (`ai-cluster benchmark --local`).
6. Benchmark distribuído de matriz com dois nós. — VERIFICADO em um host; hardware real ainda PENDENTE.
7. Comparar tempo de compute, tempo de transferência e latência total. — VERIFICADO para o caso mesmo-host (`ai-cluster benchmark --size N --repetitions R`).
8. Saída graciosa de nó com trabalho em voo. — VERIFICADO (`code/tests/phase-1-leave.sh`).
9. Dispatch de grafo de dependências (diamante). — VERIFICADO (`code/tests/phase-2-graph.sh`).

## Comparação medida — 128×128, 4 núcleos, host único

| escopo | compute médio | transferência média | nota |
|---|---|---|---|
| nó único (`--local`) | 41,365 µs | n/a | sem dispatch nem rede |
| 1 worker | 42,429 µs | 18,570 µs | compute bate com a base |
| 2 workers | 53,064 µs | 11,935 µs | compute inflado por dividir quatro núcleos |

O compute em um worker bate com a base local dentro do ruído. O número de dois workers é pior porque ambos dividem quatro núcleos. Estes números são evidência de que o harness funciona, não evidência sobre o desempenho do cluster.

## Validação Docker com dois containers

Controller e worker rodam como containers separados na rede bridge `aic-net`, então o dispatch cruza uma fronteira de rede real em vez de um socket de loopback.

```text
docker network create aic-net
docker run -d --name aic-controller --network aic-net -v aic-target:/t aic-rust:1 \
  sh -c 'exec /t/release/ai-cluster-controller'
docker run -d --name aic-worker1 --network aic-net -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=http://aic-controller:50051 -e AI_CLUSTER_NODE_ID=node-b \
  aic-rust:1 sh -c 'exec /t/release/ai-cluster-worker'
```

Observado: `node-b` se registra e chega a `READY`; 6/6 unidades de trabalho completam com `pendentes=0`; os checksums batem com os registros do controller.

### Comparação medida — 256×256, host único, dois containers

| escopo | compute médio | transferência média |
|---|---|---|
| nó único (`--local`) | 6,317 µs | n/a |
| worker em 1 container | 7,337 µs | 9,662 µs |

A transferência domina: o worker roda nos mesmos núcleos físicos que o cliente, então não há compute a economizar e cada microssegundo de transferência é custo puro. Reproduz o achado de mesmo host e confirma que transporte, serialização e enfileiramento funcionam através de uma fronteira de container. A afirmação de distribuição ainda precisa de máquinas separadas ([[f00-runbook-duas-maquinas]]).

## Próximos passos

- Substituído pelo checklist de [[fase-00-fundacao]]: a pendência real é o item 4 e o item 6 em máquinas físicas.
