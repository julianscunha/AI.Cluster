---
title: "F02 — Capability e saúde"
aliases: ["phase-2", "Fase C", "Fase C — Capability / Health", "EP C", "capability-benchmark"]
tags: [implementacao/fase/02, implementacao/estado/parcial]
type: implementacao/fase
status: parcial
---
# F02 — Capability e saúde

## Objetivo

Fazer o perfil de capability de cada nó ser **medido e reproduzível**, e o score de saúde refletir o que o nó de fato sofre (temperatura, pressão de memória e swap), para que o placement escolha nós com base em números reais e não em proxies. Ao fim, adicionar um nó produz um perfil reproduzível.

## Depende de

[[fase-01-nucleo-do-cluster]] (worker, heartbeat e registro).

## Entrega

- Discovery de hardware: CPU, RAM, storage por bloco, rede e acelerador.
- Benchmark de compute no próprio kernel das unidades, com medição de rede (sonda de upload) e storage (escrita com fsync).
- Modelo de capability em três camadas: baseline (registro), current (remedida no ocioso) e confidence.
- Modelo de saúde com liveness, pressão de memória, swap e térmica.

## Checklist

Derivado de EP §4 (C1–C4), roadmap Fase 1 (capabilities e health) e `phases.md`.

**C1 — Hardware discovery**

- [~] CPU threads, RAM e compute medido (`capability()` no worker).
- [~] Storage por bloco: NVMe/SSD/HDD por nó ([[f10-classes-de-armazenamento]]).
- [~] Rede: velocidade nominal do link sobre a referência de 1 GbE ([[f02-descoberta-de-rede]]).
- [ ] Aceleradores: sem caminho de execução no worker, nada a reportar ([[fase-16-aceleradores]]).

**C2 — Benchmark engine**

- [~] Compute medido no worker, com o matmul no próprio kernel das unidades ([[f02-benchmark-de-capability]]).
- [~] Rede medida: sonda `ProbeLink` de upload no registro, em `network_achieved_score` ([[f02-descoberta-de-rede]]).
- [~] Storage: escrita sequencial com fsync em `storage_score`; só o dispositivo do tmp, sem consumidor.
- [~] Memória: banda de cópia single-thread em `memory_bandwidth_score` ([[f02-benchmark-de-capability]]); sem consumidor no placement e sem medida sob carga concorrente.

**C3 — Capability model**

- [x] Baseline + current + confidence ([[f02-capability-corrente]]).
- [x] Distribuição do registro (mediana e desvio) para calibrar a previsão ([[f02-estabilidade-do-benchmark]]).

**C4 — Health model**

- [x] Liveness (heartbeat e grace) e score de saúde com pressão de memória, swap e térmica ([[f02-modelo-de-saude]]).
- [~] Degradação de rede: sonda repetida no ocioso (`current_network_score`); sem E2E que degrade o link.
- [ ] Storage health: sem baseline de dispositivo.

## Gate

Critério de saída: **adicionar nó produz capability profile reproduzível**.

- [~] O compute do perfil é reproduzível por benchmark. Storage e rede têm sonda de registro; a memória tem só a banda single-thread; kernels de acelerador não são medidos.

Gate padrão de fase: código ✔ · benchmark ✔ (`code/tests/phase-c-benchmark.sh`, `phase-c-current.sh`) · telemetria ✔ (`CPU_%`, `TEMP_C`, `SWAP_%`, `rede_*` no detalhe do nó) · teste de falha ✔ (taxa absurda ou benchmark quebrado cai no fallback, nunca pontua alto) · documentação ✔ · ADR: pendente de registro específico.

> [!todo] Pendente
> Memória sob carga concorrente, storage health, baseline de temperatura por hardware, latência de rede e aceleradores. Bandas de calibração por faixa de tamanho ficaram para depois de existir desvio estável.

## Slices

| slice | conteúdo | estado |
|---|---|---|
| [[f02-benchmark-de-capability]] | compute medido no worker; `compute_score` | parcial |
| [[f02-capability-corrente]] | baseline, current e confidence | implementado |
| [[f02-estabilidade-do-benchmark]] | mediana e desvio do benchmark de registro | implementado |
| [[f02-modelo-de-saude]] | score de saúde com pressão de memória, swap e térmica | implementado |
| [[f02-descoberta-de-rede]] | velocidade nominal, taxa alcançada e remedida do link | parcial |

## Códigos antigos

- EP C1, C2, C3, C4 (Fase C — Capability / Health).
- `roadmap 1` (capabilities e health).
- `phase-2/capability-benchmark`, `capability-current`, `benchmark-stability`, `health-model`, `network-discovery`.

## Specs do sistema

- [[capability-engine]]
- [[saude-e-confianca]]
- [[no-e-hardware]]
- [[camada-de-transporte]]
- [[modelo-de-desempenho]]
