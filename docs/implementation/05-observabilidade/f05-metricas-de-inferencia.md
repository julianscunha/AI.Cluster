---
title: "F05 — Métricas de inferência"
aliases: ["TTFT", "ITL", "tokens por segundo", "KV cache"]
tags: [implementacao/fase/05, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-05-observabilidade]]"
codigos-antigos: ["H1", "H4", "FR-056"]
sistema: ["[[observabilidade]]", "[[inference-api]]"]
evidencia: ["code/tests/phase-h-inference-metrics.sh"]
---
# F05 — Métricas de inferência

- [ ] Superado

## Objetivo

Responder "o chat está lento por quê?" com números: quanto demora o primeiro token, quanto demora cada token seguinte, quantos tokens por segundo e quanta memória o KV cache ocupa. Até aqui só havia métricas de Work Unit no controller; a geração roda no gateway e o controller nunca vê tokens.

## O que foi feito

- `crates/gateway/src/metrics.rs`: um `Sampler` por geração, criado antes do prompt rodar, marca cada token escolhido. O primeiro dá o **TTFT** (prompt incluído), os intervalos entre os seguintes dão o **ITL**, e tokens depois do primeiro sobre o tempo desde o primeiro dão os **tokens por segundo**. Ao fim, `finish` registra tudo e o tamanho do KV cache.
- As séries guardam as últimas 1024 amostras; p50 e p95 são nearest-rank, como no controller ([[f05-metricas]]). Contadores: requisições, falhas, tokens de prompt e tokens gerados.
- `LanguageModel::kv_bytes_per_position` (GPT-2 e Llama): camadas × 2 × largura de K/V × 8 bytes. O tamanho do cache é posições (prompt + gerados) vezes isso; o gateway guarda o último e o máximo.
- `GET /metrics` no gateway, em texto Prometheus, atrás da mesma chave Bearer da API (401 sem ela). Vale para as duas rotas de chat (inteira e SSE).

## Evidência

`bash code/tests/phase-h-inference-metrics.sh` (precisa de `code/models/gpt2`): três gerações de 5 tokens dão `requests_total 3`, `generated_tokens_total 15`, TTFT p50 0,23 s, ITL p50 0,09 s, ~9,6 tokens/s, KV cache de 1.474.560 bytes (10 posições × 147.456). Unitários em `metrics.rs`: percentis, separação entre TTFT e intervalos, e a saída em texto.

## Limites

> [!warning] Só o gateway, só instantâneo
> As métricas vivem na memória do gateway: um restart as zera, e não há série temporal. O painel `ai-cluster dashboard` ainda não as lê (ele fala só com o controller). A banda do KV cache é a do final da geração, não a ocupação concorrente de várias requisições. Cancelamentos e timeouts contam como falha. Tokens evitados e tokens em cache não existem porque não há prefix cache.

## Próximos passos

- Mostrar estas séries no `dashboard`, com `--gateway <url>`.
- Persistir ou exportar histórico quando houver um coletor.
