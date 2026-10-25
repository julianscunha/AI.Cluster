---
title: "Edge Optimizer"
aliases: ["Otimizador de borda", "docs/15", "15 — Edge Optimizer"]
tags: [sistema/subsistema/ia]
type: sistema/subsistema
status: proposto
implementacao: nao-iniciado
fases: ["[[fase-14-edge-optimizer]]"]
---

# Edge Optimizer

O Edge Optimizer reduz trabalho, tokens, contexto, latência e comunicação **antes** da inferência pesada. Roda depois da validação inicial da [[request-manager]] e antes da alocação de recursos. Voltar para [[ia]].

## Pipeline

```text
Request
 -> classificação
 -> cache exato
 -> cache semântico
 -> graph retrieval ([[graph-engine]])
 -> seleção de contexto
 -> deduplicação
 -> compressão
 -> resolução de skill/tool
 -> compilação do prompt
 -> token budget
 -> model routing
 -> LLM ([[inference-engine]])
```

## Caminho rápido

Requisição determinística ou cacheável não chega ao LLM: cálculo simples, consulta de estado do cluster, cache hit.

## Componentes

- **Retrieval assistido por grafo:** usa entidades e relações para chegar a fatos, incidentes, dependências e contexto relevantes.
- **Compressão:** guiada pela pergunta; preserva a informação relevante e indica confiança.
- **Resolução de skill/tool:** decide fora do contexto principal quais capabilities são necessárias e envia só os schemas relevantes.
- **Compilador de prompt:** transforma contexto disperso numa representação mínima adequada à tarefa.
- **Token budgeter:** estima entrada, saída, máximo, contexto disponível e objetivo de qualidade.
- **Model routing:** escolhe a variante mais barata que atende ao objetivo de qualidade, dentro da política de fallback ([[inference-api]]).

## Decisão de otimização

Cada otimização tem custo próprio. Ela só é aplicada quando a economia esperada supera esse custo. Uma redução de contexto pode tornar executável uma request antes inviável; o orçamento é recalculado depois da otimização.

## Métricas

Tokens originais, otimizados e evitados; razão de redução; chamadas ao LLM evitadas; compute evitado; taxa de acerto de cache; taxa de acerto do grafo. Essas métricas alimentam [[observabilidade]].

> [!info] O que já existe (MVP)
> Nenhuma etapa de otimização de borda existe. Não há classificação, cache de respostas, retrieval, compressão, resolução de skill nem roteamento por custo. O gateway não altera a entrada do usuário além de validar tamanho e montar o template de chat do modelo.
>
> Há dois caches, mas são de outra natureza: o cache de fatias de pesos do worker (`ShardStore`, LRU, [[f08-canal-residente-de-fatias]]) e o KV cache de uma geração no gateway ([[f08-engine-local]]). Nenhum deles reutiliza respostas entre requisições.

> [!todo] Falta
> - Cache exato e semântico de respostas, com invalidação.
> - Caminho rápido para consultas determinísticas do cluster.
> - Retrieval com grafo, seleção de contexto e deduplicação.
> - Compressão orientada pela pergunta.
> - Resolução de tools e compilação de prompt.
> - Token budgeter e roteamento de modelo por custo e qualidade.
> - Medição de ganho contra custo de cada etapa ([[fase-14-edge-optimizer]]).
