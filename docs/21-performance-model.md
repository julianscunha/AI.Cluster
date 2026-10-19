# 21 — Performance Model

## Objective

Prever custo e capacidade e comparar previsão com resultado observado.

## Inputs

- hardware benchmark;
- historical workload metrics;
- node load;
- network topology;
- storage performance;
- model/variant;
- precision;
- context size.

## Outputs

- latency prediction;
- throughput prediction;
- node cost;
- cluster capacity;
- confidence;
- expected variance.

## Learning loop

```text
benchmark
 -> real workload
 -> observe
 -> compare expected/actual
 -> update model
 -> improve next plan
```

## Cluster capacity

Manter separadamente:

- latency capacity;
- throughput capacity;
- memory capacity;
- storage capacity;
- network capacity.

Não tratar soma de RAM/CPU como se fosse um único dispositivo físico.
