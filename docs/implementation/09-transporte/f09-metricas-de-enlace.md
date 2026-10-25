---
title: "F09 — Métricas de enlace"
aliases: ["E5", "latência de rede"]
tags: [implementacao/fase/09, implementacao/fatia, implementacao/estado/parcial]
type: implementacao/fatia
status: parcial
superado: false
fase: "[[fase-09-transporte]]"
codigos-antigos: ["E5"]
sistema: ["[[camada-de-transporte]]", "[[saude-e-confianca]]"]
evidencia: ["code/tests/phase-c-network-degrade.sh"]
---
# F09 — Métricas de enlace

- [ ] Superado

## Objetivo

O enlace entre worker e controller era medido só em vazão de upload ([[f02-descoberta-de-rede]]). A latência, a outra metade do custo de uma operação pequena, não era medida embora o campo `HealthReport.network_latency_ms` existisse no wire desde o início.

## O que foi feito

- O worker mede a latência com cinco `ProbeLink` de payload vazio e fica com o menor tempo (em milissegundos), pelo mesmo motivo do melhor-de-N dos outros testes: contenção só atrasa. O payload vazio mantém a banda fora do número.
- A medição roda no ramo ocioso, junto da vazão remedida (`AI_CLUSTER_RECOMPUTE_INTERVAL_SECS`), e viaja no `HealthReport.network_latency_ms`. Falha de sonda mantém o valor anterior; zero é "não medido".
- A CLI mostra `latencia_ms=` no `node show`.

## Evidência

`bash code/tests/phase-c-network-degrade.sh` confere que `node show` traz `latencia_ms=` preenchido (0,645 ms num nó em contêiner na mesma rede). O estrangulamento do `tc` limita taxa, não atraso, então ele não muda a latência de um probe vazio.

## Limites

> [!warning] Latência até o controller, sem perda
> A latência é worker→controller, não entre dois workers. Perda e congestionamento não são medidos: sobre TCP a perda aparece como atraso, não como falha, e uma sonda que falha só mantém a medida anterior. A latência ainda não entra no custo do placement nem no score de saúde; o termo de transferência continua só de vazão.

## Próximos passos

- Usar a latência no custo de unidades pequenas e na decisão entre poll e canal residente.
- Medir latência entre workers quando existir tráfego direto entre eles.
