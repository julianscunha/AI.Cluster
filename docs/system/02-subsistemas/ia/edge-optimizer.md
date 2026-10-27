---
title: "Edge Optimizer"
aliases: ["Otimizador de borda", "Edge Optimizer"]
tags: [sistema/subsistema/ia]
type: sistema/subsistema
---

# Edge Optimizer

O Edge Optimizer reduz trabalho, tokens, latência e comunicação antes da inferência pesada. Hoje existe uma parte: o cache exato de respostas. O restante do pipeline é planejado. Voltar para [[ia]].

## Cache exato de respostas

A geração é gulosa. Com o mesmo prompt, os mesmos pesos e o mesmo limite de tokens, a resposta é sempre a mesma. Por isso o gateway guarda a resposta e a devolve sem tocar no modelo.

- **Chave:** tenant, sequência completa de tokens do prompt, modelo, versão dos pesos e `max_tokens`. Qualquer diferença é outra chave.
- **Tamanho:** `AI_CLUSTER_CACHE_ENTRIES` entradas (padrão 256), com descarte do menos usado recentemente. `0` desliga.
- **Armazenamento:** só uma geração completa vai para o cache. Geração cancelada ou com erro não.
- **Hit:** não espera a vaga de geração. Reproduz os tokens guardados, inteiros ou em stream.
- **Cota:** o hit é cobrado ao tenant como uma geração ([[request-manager]]).
- **Métricas:** `ai_cluster_cache_hits_total`, `ai_cluster_cache_misses_total`, `ai_cluster_cache_tokens_avoided_total` e `ai_cluster_cache_entries` no `/metrics` do gateway ([[observabilidade]]).
- **Custo:** uma busca é da ordem de microssegundos, contra centenas de milissegundos de uma geração.

"Exato" quer dizer que não há semelhança envolvida. Um prompt reescrito não acerta.

## Limites conhecidos

- **Por processo.** O cache some no reinício e não é compartilhado entre gateways.
- **Por tenant.** Um tenant nunca recebe a resposta que outro perguntou. Isso reduz acertos quando tenants repetem as mesmas perguntas.
- **Busca linear** na ordem de uso. Com o padrão de 256 entradas, o custo é desprezível.
- **Só vale para geração gulosa.** Com amostragem, a chave precisaria de seed, ou o cache não se aplica.

> Planejado: cache semântico, com acerto para perguntas parecidas.
> Planejado: pipeline completo de otimização antes do modelo: classificação, retrieval com grafo ([[graph-engine]]), seleção e deduplicação de contexto, compressão guiada pela pergunta, compilação de prompt e resolução de ferramentas.
> Planejado: token budgeter, que estima entrada, saída e contexto disponível.
> Planejado: roteamento de modelo pela variante mais barata que atende ao objetivo de qualidade.
> Planejado: métricas de tokens original, otimizado e evitado, com a razão de redução.
> Planejado: decisão por custo: cada etapa só roda quando a economia prevista supera o custo dela.
