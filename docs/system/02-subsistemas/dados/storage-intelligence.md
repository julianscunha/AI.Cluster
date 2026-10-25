---
title: "Storage Intelligence"
aliases: ["Inteligência de armazenamento", "docs/18", "18 — Storage Intelligence"]
tags: [sistema/subsistema/dados]
type: sistema/subsistema
status: proposto
implementacao: parcial
fases: ["[[fase-10-inteligencia-de-storage]]"]
---

# Storage Intelligence

O armazenamento é tratado como recurso complementar à memória e à execução. O comportamento se adapta ao tipo e à saúde de cada dispositivo. Voltar para [[dados]].

## Detecção

Por dispositivo: tipo (HDD, SATA SSD, NVMe), capacidade, vazão de leitura e escrita, latência, temperatura, saúde e, quando disponível, desgaste e endurance.

## Papéis por tipo

| Dispositivo | Papel |
|---|---|
| HDD | Armazenamento frio, cache limitado, swap conservador |
| SSD | Swap, cache, staging, checkpoint |
| NVMe | Cache quente agressivo, staging, checkpoint, cache de tensores |

As políticas são dinâmicas: mudam com a carga e com a saúde.

## Alocação de espaço

Reservar, antes de qualquer uso: OS e recuperação, swap, cache de modelo, cache de tensores, checkpoint e estado, e reserva de emergência. Não consumir 100% do espaço.

## Política de SSD

Em SSD ou NVMe saudável e rápido, uma parcela grande da capacidade pode ser usada para swap e cache quando isso melhora o cluster. Respeita espaço livre, endurance e folga térmica.

## Integração com a saúde

Em degradação: reduzir escrita pesada, evitar checkpoints agressivos, diminuir a dependência de swap, migrar dados quando possível e alertar o controller ([[saude-e-confianca]]).

Papel na hierarquia de estado: HDD é COLD, SSD e NVMe são WARM, RAM é HOT ([[state-manager]], [[adr-005-camadas-de-storage]]).

> [!info] O que já existe (MVP)
> - Cada worker detecta as classes de armazenamento lendo `/sys/block/*/queue/rotational` e o prefixo `nvme` do nome. O resultado é `storage_classes` no relatório de capacidade, da classe mais quente para a mais fria (`nvme`, `ssd`, `hdd`). Código: `code/crates/worker/src/main.rs`.
> - `AI_CLUSTER_STORAGE_CLASSES` substitui a detecção; `ai-cluster nodes` mostra a coluna `ARMAZ`.
> - Limites: NVMe e SATA SSD ficam juntos em WARM; não há medida de latência nem vazão; não há capacidade por classe; um nó com dois NVMe reporta só `nvme`; dentro de contêiner, `/sys/block` mostra os discos do host.
> - Nada no cluster coloca estado em disco por camada. O cache de fatias dos workers é só RAM ([[f08-canal-residente-de-fatias]]).
>
> Slice: [[f10-classes-de-armazenamento]].

> [!warning] Desvio do alvo e migração
> A classificação existe, mas nenhuma decisão de alocação a usa. Os pesos dos modelos são lidos do disco sem política de camada, e o cache de fatias é só RAM (LRU, `AI_CLUSTER_SHARD_CACHE_MB`). Migração: alocação por camada em [[fase-10-inteligencia-de-storage]] e cache de modelo HOT/WARM/COLD em [[model-manager]] ([[fase-12-model-runtime]]).

> [!todo] Falta
> - Benchmark de storage (vazão, latência) para separar NVMe de SATA SSD e ordenar nós dentro de uma camada.
> - Capacidade por dispositivo e contagem por dispositivo.
> - Reservas de OS, swap, cache, checkpoint e emergência.
> - Medidas de SMART, temperatura e desgaste.
> - Política de escrita por saúde e alerta ao controller ([[saude-e-confianca]]).
> - Swap e cache de SSD/NVMe conforme política.
