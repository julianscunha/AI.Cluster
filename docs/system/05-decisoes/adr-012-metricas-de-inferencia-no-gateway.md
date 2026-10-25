---
title: "ADR-012 — Métricas de inferência no gateway"
aliases: ["ADR-012", "Métricas de inferência"]
tags: [sistema/adr, subsistema/operacao]
type: sistema/adr
status: decidido
implementacao: implementado
fases: ["[[fase-05-observabilidade]]"]
---

# ADR-012 — Métricas de inferência no gateway

Data: 2026-10-25 · Voltar para [[decisoes]].

## Contexto

As métricas da F05 foram feitas para Work Units, no controller. A geração de texto roda no gateway, que decide cada token; o controller só vê as multiplicações de fatia que o gateway lhe manda, sem noção de token, prompt ou cache. FR-056 pede TTFT, ITL, tokens por segundo e latência.

## Decisão

1. **Quem escolhe o token mede o token.** O gateway cronometra cada geração com um `Sampler` e expõe o resultado em `GET /metrics` (Prometheus). O controller não ganha RPC nem campos novos para isso.
2. **TTFT inclui o prompt.** O relógio começa antes do prefill; o primeiro token carrega o custo de ler o prompt, que é o que o cliente sente.
3. **ITL e tokens/s só contam o decode.** Os intervalos entre tokens e a taxa depois do primeiro excluem o prefill, para que a taxa de decode seja comparável entre prompts de tamanhos diferentes.
4. **Percentis sobre as últimas 1024 amostras.** Nearest-rank, como no controller. Descrevem a carga recente, não a vida inteira do processo.
5. **Mesma credencial da API.** `/metrics` do gateway exige a chave Bearer; não abre papel novo.
6. **KV cache por conta, não por medição.** Bytes por posição vêm do modelo (`kv_bytes_per_position`) e o tamanho é posições × isso. Exato para os dois modelos, sem inspecionar o cache vivo.

## Consequências

- Dois pontos de coleta (controller e gateway) para quem monta um painel. O `dashboard` ainda não lê o gateway.
- Um restart do gateway zera as séries; não há histórico.
- O trabalho do cluster dentro de uma geração (tempo das chamadas aos workers) continua nos logs do gateway (`remoto: ...`), não nas métricas.

## Alternativas

- Enviar eventos de token ao controller: acopla o caminho quente de geração a uma RPC e dá ao controller um conceito que ele não usa.
- Medir pelo lado do cliente: depende de cada cliente e perde o prefill do lado do servidor.
- Histograma Prometheus em vez de percentis calculados: melhor para agregar entre gateways, mas exige escolher buckets antes de existirem dados.

> [!todo] Painel, histórico e agregação entre vários gateways ficam para quando houver mais de um gateway ou um coletor ([[fase-11-request-manager]]).
