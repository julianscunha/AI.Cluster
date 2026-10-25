---
title: "F14 — Cache exato de respostas"
aliases: ["M1", "cache exato"]
tags: [implementacao/fase/14, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-14-edge-optimizer]]"
codigos-antigos: ["M1"]
sistema: ["[[edge-optimizer]]", "[[inference-api]]"]
evidencia: ["code/tests/phase-o-cache.sh"]
---
# F14 — Cache exato de respostas

- [ ] Superado

## Objetivo

A geração é gulosa: o mesmo prompt sobre os mesmos pesos dá os mesmos tokens. Repetir o trabalho de uma pergunta já respondida é custo puro. Esta fatia (M1 do edge optimizer) guarda a resposta no gateway e a devolve da memória na segunda vez, sem usar o modelo.

## O que foi feito

- `crates/gateway/src/cache.rs`: `ResponseCache`, LRU por número de entradas (`AI_CLUSTER_CACHE_ENTRIES`, padrão 256; `0` desliga). A chave é o tenant, a sequência inteira de tokens do prompt, o modelo, a **versão dos pesos** ([[f12-manifest-de-modelo]]) e o `max_tokens`: "exato" quer dizer que não há semelhança nenhuma envolvida, e qualquer diferença é um erro de cache.
- `prepare` tokeniza antes de entrar na fila e consulta o cache. Um acerto **não espera o slot de geração**; só um erro entra na fila por classe ([[f11-tenants-quotas-e-fila]]).
- `generate_or_replay`, usado pelo chat inteiro e pelo streaming, reproduz os tokens do cache ou gera de verdade. Nos dois casos cobra o tenant (prompt mais tokens visíveis), então a cota não vira brecha. Só uma geração real e completa vai para o cache: cancelada ou com erro não.
- Métricas no `/metrics` do gateway: `ai_cluster_cache_hits_total`, `_misses_total`, `_tokens_avoided_total` e `_entries`. As métricas de geração (TTFT, ITL, tokens/s) só contam gerações reais.

## Evidência (ganho contra custo, o gate da fase)

`bash code/tests/phase-o-cache.sh` (precisa de `code/models/gpt2`): um pedido de 20 tokens leva **1,85 s** gerado e **2,4 ms** do cache, com o mesmo texto; o streaming de um acerto entrega o mesmo texto da resposta inteira; outro `max_tokens` gera de novo; o `/metrics` mostra 2 acertos, 2 erros e 40 tokens poupados. O custo do cache é o de uma busca: o teste `a_lookup_costs_microseconds_next_to_a_generation` mede uma busca com prompt de 512 tokens num cache cheio de prompts assim e exige menos de 5 ms (na prática microssegundos), contra centenas de milissegundos de uma geração.

## Limites

> [!warning] O que o cache não é
> Não é semântico (M2): uma pergunta reescrita não acerta. É por processo do gateway: some no restart e não é compartilhado entre vários gateways. Vale só enquanto a geração for gulosa; a primeira amostragem com `temperature` torna a chave insuficiente (precisa de seed na chave) ou o cache inválido para esses pedidos. Cada tenant tem as suas entradas: um acerto rápido nunca diz o que outro tenant perguntou (o preço é menos acertos quando tenants repetem as mesmas perguntas). A busca é linear na ordem de uso (256 entradas).

## Próximos passos

- Cache semântico (M2) e deduplicação (M5), só se a medição mostrar acerto que compense o custo.
- Chave com seed quando houver amostragem.
- Cache compartilhado entre tenants para prompts públicos, se o canal de tempo for aceitável.
