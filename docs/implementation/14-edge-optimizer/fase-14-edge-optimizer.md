---
title: "F14 — Edge optimizer"
aliases: ["Fase M — Edge Optimizer", "docs/15 (edge optimizer)", "M5"]
tags: [implementacao/fase/14, implementacao/estado/parcial]
type: implementacao/fase
status: parcial
codigos-antigos: ["EP M1–M10", "roadmap 5 (exact/semantic cache, dedup, context selection, token budgeting)", "M5"]
sistema: ["[[edge-optimizer]]", "[[request-manager]]", "[[graph-engine]]", "[[inference-api]]"]
---
# F14 — Edge optimizer

> [!warning] Colisão de "M"
> Os itens M1–M10 do EP são do edge optimizer. Os marcos M0–M7 de [[marcos]] são outra numeração. O marco M5 corresponde a esta fase.

## Objetivo

Reduzir tokens e latência de inferência com caches, recuperação, deduplicação, compressão, seleção de contexto e orçamento de tokens. Cada técnica precisa mostrar que o ganho supera o custo do próprio otimizador.

## Depende de

- [[fase-11-request-manager]]: orçamento de tokens e admissão.
- [[fase-12-model-runtime]]: execução do modelo que se quer poupar.
- [[fase-13-graph-engine]]: recuperação por grafo (M3).

## Entrega

- Caches exato e semântico (M1, M2).
- Recuperação por grafo, seleção de contexto e deduplicação (M3–M5).
- Compressão, resolução de skill e tool, compilador de prompt, orçamento de tokens e caminho rápido (M6–M10).

## Checklist

Verificado em `code/crates`: M1 feito; os demais não iniciados.

- [x] M1 Cache exato. LRU no gateway; 1,85 s gerado contra 2,4 ms do cache, com o mesmo texto ([[f14-cache-exato]]).
- [ ] M2 Cache semântico.
- [ ] M3 Recuperação por grafo.
- [ ] M4 Seleção de contexto.
- [ ] M5 Deduplicação.
- [ ] M6 Compressão.
- [ ] M7 Resolução de skill e tool.
- [ ] M8 Compilador de prompt.
- [ ] M9 Orçamento de tokens.
- [ ] M10 Caminho rápido.
- [~] Critério de saída: redução de tokens ou latência que supere o custo do otimizador, medida. Medido para M1 (acerto 750 vezes mais rápido que gerar, busca em microssegundos); M2 a M10 sem medição. M3 depende de [[fase-13-graph-engine]], que não foi iniciada de propósito.

## Gate

Gate padrão (EP §19): código, benchmarks, telemetria, testes de falha, atualização de documentação e ADR quando a arquitetura mudar.

Gate específico (manifesto): ganho maior que o custo; M5 completo com benchmark de redução de tokens, de cache hit, de recuperação de grafo e do overhead do otimizador.

## Slices

Notas desta pasta (esta nota é o MOC):

- [[f14-cache-exato]]: M1, cache exato de respostas no gateway.

Slices a criar:

| Slice | Item do EP |
|---|---|
| `f14-cache-exato` | M1 |
| `f14-cache-semantico` | M2 |
| `f14-recuperacao-por-grafo` | M3 |
| `f14-selecao-de-contexto` | M4 |
| `f14-deduplicacao` | M5 |
| `f14-compressao` | M6 |
| `f14-resolucao-de-skill-e-tool` | M7 |
| `f14-compilador-de-prompt` | M8 |
| `f14-orcamento-de-tokens` | M9 |
| `f14-caminho-rapido` | M10 |

## Códigos antigos

- EP M1–M10.
- Roadmap 5, itens de cache, dedup, seleção de contexto e orçamento de tokens.
- Marco M5 (`[ ]`, Fase 5 do roadmap).
- Pacote da Fase 5 em `phases.md`: exact cache, semantic cache, compression evaluator.

## Specs do sistema

- [[edge-optimizer]]: subsistema-alvo.
- [[request-manager]]: orçamento de tokens consumido pelo otimizador.
- [[graph-engine]]: fonte de recuperação (M3).
- [[inference-api]]: ponto onde o otimizador atua.
