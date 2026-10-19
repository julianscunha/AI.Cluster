# 13 — State Manager

## Objective

Preservar o estado necessário para retomar uma execução quando placement ou hardware mudarem.

## State classes

1. Model State.
2. Execution State.
3. KV Cache.
4. Checkpoint/Recovery State.

## Tiers

```text
HOT  -> RAM / VRAM
WARM -> NVMe / SSD
COLD -> persistent/distributed storage
```

## Replication

Não replicar tudo 1:1. Priorizar:

- metadata crítica;
- execution state;
- KV cache seletivo;
- checkpoint conforme política.

Caches derivados podem ser descartáveis.

## State directory

Referências lógicas:

```text
state://req-1842/kv/layer-42
```

O directory mantém primary/replica/localization.

## Leases

State ownership deve usar leases/heartbeats para identificar rapidamente perda de owner.

## Checkpoints

Preferir checkpoints incrementais e em pontos de execução seguros; não persistir toda a memória viva a cada token.

## Recovery

```text
node lost
 -> identify state
 -> promote replica / recompute
 -> restore
 -> regenerate placement
 -> continue
```
