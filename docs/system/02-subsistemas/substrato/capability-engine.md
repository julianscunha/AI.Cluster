---
title: "Motor de capacidade"
aliases: ["Motor de capacidade", "Capability Engine", "docs/05", "05 — Capability Engine", "05 — Motor de capacidade"]
tags: [sistema/subsistema/substrato]
type: sistema/subsistema
status: decidido
implementacao: parcial
fases: ["[[fase-02-capability-e-saude]]"]
---

# Motor de capacidade

Mede a capacidade real do hardware e converte o resultado em um perfil que o planner consome. Não usa um score global: cada dimensão fica separada, porque um nó pode ser excelente em compute e fraco em rede.

## Baseline

Medido no registro do nó. O benchmark inicial cobre:

- compute geral;
- matmul;
- banda de memória;
- throughput e latência de armazenamento;
- latência e banda de rede;
- kernels de acelerador disponíveis;
- kernels específicos de workload.

## Capacidade dinâmica

A capacidade efetiva é ajustada conforme:

- throttling térmico;
- pressão de memória;
- carga atual;
- degradação de rede;
- degradação de armazenamento;
- política de energia.

## Perfil

```text
CapabilityProfile
  general_compute
  matmul_int4
  matmul_int8
  fp16
  memory
  memory_bandwidth
  gpu
  npu
  network
  storage
  thermal_headroom
```

Cada dimensão tem baseline, medida corrente e confiança. Um score único esconde gargalos, por isso não é usado para decisão.

## Modelo de custo

`estimated_cost(work, node)` é função do workload e do estado atual do nó, não só do hardware. O planner compara custo entre nós com esse valor.

## Confiança

Toda estimativa carrega uma medida de confiança derivada da quantidade e da qualidade dos dados históricos. Sem dados suficientes, a estimativa vale pouco e não deve ser tratada como medida.

> [!info] O que já existe
> Campos em `CapabilityReport` de `code/proto/cluster.proto`, medidos em `code/crates/worker/src/main.rs`:
>
> | Dimensão | Campo | Como é medido hoje |
> |---|---|---|
> | Compute | `compute_score`, `compute_median_score`, `compute_deviation`, `compute_confidence` | Matmul 256×256 em `f64` (`code/crates/common/src/matrix.rs`), 5 execuções. Score = melhor execução, normalizado por `REFERENCE_CORE_GFLOPS` = 6.0 (um núcleo de referência vale 1.0). Confiança cai quando as execuções divergem. |
> | Compute corrente | `current_compute_score`, `compute_confidence` | Remedido com o nó ocioso a cada 60 s (`AI_CLUSTER_RECOMPUTE_INTERVAL_SECS`). |
> | Rede | `network_score` | Velocidade nominal do link (`/sys/class/net`), relativa a 1 GbE. |
> | Rede alcançada | `network_achieved_score`, `current_network_score` | Upload medido pelo `ProbeLink` (2 MiB, 3 execuções, melhor); remedido no ocioso. |
> | Armazenamento | `storage_classes`, `storage_score` | Classes `nvme`/`ssd`/`hdd` de `/sys/block`. Escrita de 16 MiB com fsync, relativa a 500 MB/s. Mede só o dispositivo do diretório temporário. |
> | Memória | `memory_bytes` | `MemTotal` de `/proc/meminfo`. |
> | Threads | `cpu_threads` | Núcleos reportados, limitados aos disponíveis no host. |
>
> Previsão de tempo: `predicted_compute_us` em `code/crates/controller/src/placement.rs` usa `flops / taxa` com a razão típico/melhor e um custo fixo por unidade. Parte de `estimated_cost` existe só para compute; o termo de transferência é heurístico.
>
> Pinos para teste: `AI_CLUSTER_COMPUTE_SCORE`, `AI_CLUSTER_NETWORK_SCORE`. Um nó com score fixado reporta `0` em mediana e desvio, porque zero significa "não medido".
>
> Fases: [[f02-benchmark-de-capability]], [[f02-estabilidade-do-benchmark]], [[f02-capability-corrente]], [[f02-descoberta-de-rede]], [[f04-placement]].

> [!todo] Medidas que faltam no perfil-alvo: banda de memória (`memory_bandwidth_score` existe no proto, mas o worker não o preenche); matmul int4, int8 e fp16; GPU, NPU e kernels de acelerador; `thermal_headroom` como dimensão própria. Aceleradores: [[fase-16-aceleradores]].

> [!warning] Divergência: o campo `precisions` reporta só `fp32`, mas o kernel de matmul calcula em `f64`. O perfil não distingue precisão de cálculo. Dimensões de capacidade dinâmica (carga, pressão de memória, energia) entram só pela saúde do nó ([[saude-e-confianca]]), não pelo perfil.
