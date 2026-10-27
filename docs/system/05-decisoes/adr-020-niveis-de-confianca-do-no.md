---
title: "ADR-020 — Níveis de confiança do nó: pendente, aprovado e atestado"
aliases: ["ADR-020", "confiança", "atestado"]
tags: [sistema/adr, subsistema/seguranca]
type: sistema/adr
---

# ADR-020 — Níveis de confiança do nó: pendente, aprovado e atestado

Status: decidido · Data: 2026-10-27 · Voltar para [[decisoes]].

## Contexto

A pendência pedia níveis `UNKNOWN` até `TRUSTED`, dependendo de um fluxo de enrollment. Hoje o nó nasce **pendente** (não recebe trabalho) e o operador o **aprova**; há quarentena automática por resultado errado e, desde a release assinada ([[adr-018-release-assinada-e-medida-do-slot]]), a marca **atestado**: o nó roda a release desejada e a medida do slot confere com a assinada.

## Decisão

1. **Três níveis, derivados do que já existe, sem fluxo de enrollment novo:** `pendente` (registrado, não aprovado), `aprovado` (`trusted`) e `atestado` (aprovado e com a medida do slot conferida). `UNKNOWN` é só "id que o controller nunca viu" e não é um estado guardado.
2. **O nível é uma exigência da unidade, não do nó.** `WorkUnit.require_attested` (e `EnqueueWorkRequest.require_attested`, `--require-attested` no `benchmark`) faz o controller entregar a unidade só a nós atestados. Pendente nunca recebe trabalho, com ou sem a flag.
3. **O conjunto de atestados é derivado ao vivo** (identidade do nó contra a release publicada), refeito só enquanto uma unidade que exige atestado espera, e nunca persistido. Depois de um reinício ele se refaz quando os nós se registram de novo.
4. **No ranking, a unidade com exigência compete só entre atestados:** um nó melhor porém não atestado nunca poderia recebê-la e não pode fazer o nó atestado ceder.

## Consequências

- **Sem release publicada ninguém é atestado**, e uma unidade com a exigência espera para sempre. É o comportamento seguro, e o operador vê a unidade parada na fila.
- A atestação é **só de software**: um nó comprometido mente sobre a medida. Atestação de hardware (TPM, secure boot) segue nas pendências de F15.
- Um fluxo de enrollment (token de uso único por nó, aprovação automática por regra) continua possível sobre estes níveis, sem mudar o protocolo.

## Alternativas

- Níveis numéricos de 0 a 2 em `min_trust`: descartada. `eligible()` já exclui o pendente, então na prática é um booleano, e um enum sem terceiro valor com sentido só pesa o protocolo.
- Exigir atestado de todos os nós por padrão: descartada. Quebraria nós de teste e containers, que nunca têm medida de slot.
