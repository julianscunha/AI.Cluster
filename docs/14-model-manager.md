# 14 — Model Manager / Registry

## Objective

Gerenciar modelos, versões, variantes de precisão e shards independentemente de placement.

## Registry entities

- Model;
- Version;
- Variant;
- Manifest;
- Artifact;
- Shard;
- Compatibility profile.

## Model variant

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

## Immutable execution reference

Uma execução referencia uma variante/version exata e não deve alterá-la arbitrariamente no meio do request.

## Model cache

Localizações:

- RAM/VRAM: hot;
- NVMe/SSD: warm;
- repository: cold.

## Placement

Model Manager conhece onde os shards estão. Execution Planner decide onde deveriam estar.

## Prefetch

Pré-carregar o próximo shard quando houver previsibilidade suficiente.

## Compatibility

Cada shard/variant deve declarar arquitetura, memória mínima, precisão e backends suportados.
