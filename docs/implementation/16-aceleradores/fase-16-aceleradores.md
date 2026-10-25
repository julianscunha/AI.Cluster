---
title: "F16 — Aceleradores"
aliases: ["Fase O — Accelerators", "M6"]
tags: [implementacao/fase/16, implementacao/estado/planejado]
type: implementacao/fase
status: planejado
codigos-antigos: ["EP O", "roadmap 7 (GPU, ARM64, NPU)", "M6"]
sistema: ["[[no-e-hardware]]", "[[capability-engine]]", "[[worker-runtime]]"]
---
# F16 — Aceleradores

## Objetivo

Acrescentar backends de aceleração (ARM64, GPU NVIDIA, GPU AMD e Intel, NPU) ao cluster. Cada backend entra depois da descoberta de capacidade e de um benchmark próprio.

## Depende de

- [[fase-02-capability-e-saude]]: descoberta de aceleradores (pendente em F02).
- [[fase-12-model-runtime]]: backends de execução de modelo.
- [[fase-15-ai-cluster-os]]: imagem ARM64 e multi-arquitetura.

## Entrega

Backends na ordem do EP: CPU x86_64 → CPU ARM64 → GPU NVIDIA → GPU AMD/Intel → abstração de NPU → NPU móvel. Cada um com capability discovery e benchmark próprio.

## Checklist

Verificado em `code/proto/cluster.proto` e `code/crates/worker/src/main.rs`.

- [x] CPU x86_64. Backend de matmul e engine CPU em uso no cluster de contêineres.
- [ ] CPU ARM64. Não existe (nenhuma referência a `aarch64` ou `target_arch` em `code/crates`).
- [ ] GPU NVIDIA. Não existe.
- [ ] GPU AMD ou Intel. Não existe.
- [ ] Abstração de NPU. Não existe.
- [ ] NPU móvel. Não existe.
- [~] Descoberta de aceleradores. `CapabilityReport.accelerators` existe no proto, mas o worker nunca o preenche; `precisions` é `fp32` fixo.
- [ ] Critério de saída: cluster x86 + ARM com scores por workload (M6). Não atendido.

## Gate

Gate padrão (EP §19): código, benchmarks, telemetria, testes de falha, atualização de documentação e ADR quando a arquitetura mudar.

Gate específico (manifesto): M6 completo, com cluster x86 + ARM, capacidades diferentes e scheduler usando scores por workload. Cada backend passa pelo próprio benchmark antes de entrar no placement.

## Slices

Notas desta pasta (esta nota é o MOC): nenhum slice criado.

Slices a criar:

- `f16-cpu-arm64`.
- `f16-gpu-nvidia`.
- `f16-gpu-amd-intel`.
- `f16-abstracao-de-npu`.
- `f16-npu-movel`.

## Códigos antigos

- EP O.
- Roadmap 7, itens GPU backend, ARM64 backend e NPU plugin architecture.
- Marco M6 (`[ ]`, depende do scheduler de [[fase-02-capability-e-saude]]).

## Specs do sistema

- [[no-e-hardware]]: inventário de hardware por nó.
- [[capability-engine]]: descoberta e benchmark por capacidade.
- [[worker-runtime]]: runtime do worker onde os backends rodam.
