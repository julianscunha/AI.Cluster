---
title: "14 — Model Manager / Registry"
type: spec
status: draft
tags: [spec, model-manager]
---

# 14 — Model Manager / Registry

## Objetivo

Gerenciar modelos, versões, variantes de precisão e shards independentemente de placement.

## Entidades do registry

- Model;
- Version;
- Variant;
- Manifest;
- Artifact;
- Shard;
- Compatibility profile.

## Variante de modelo

O mesmo modelo pode possuir:

```text
FP16
FP8
INT8
Q8
Q6
Q5
Q4
```

A quantização é um atributo/artefato, não necessariamente outro modelo conceitual.

## Referência de execução imutável

Uma execução referencia uma variante/version exata e não deve alterá-la arbitrariamente no meio do request.

## Cache de modelo

Localizações:

- RAM/VRAM: hot;
- NVMe/SSD: warm;
- repository: cold.

## Placement

Model Manager conhece onde os shards estão. Execution Planner decide onde deveriam estar.

## Prefetch

Pré-carregar o próximo shard quando houver previsibilidade suficiente.

## Compatibilidade

Cada shard/variant deve declarar arquitetura, memória mínima, precisão e backends suportados.
