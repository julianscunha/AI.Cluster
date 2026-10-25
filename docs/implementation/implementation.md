---
title: "Implementação"
aliases: ["Planejamento", "Fases de implementação", "Roadmap", "EXECUTION-PLAN"]
tags: [implementacao/moc, moc]
type: implementacao/moc
status: vivo
superado: false
---

# Implementação

Planejamento e execução, organizados em **fases por dependência** para não refazer trabalho: cada fase só começa quando as anteriores de que depende entregaram o seu gate. Estado consolidado em [[status]]; equivalência com os nomes antigos em [[equivalencia-de-fases]].

- [ ] Superado

## Fases

Legenda: `[x]` feito · `[~]` parcial · `[ ]` não iniciado.

| Fase | Nota | Depende de | Estado |
|---|---|---|---|
| F00 | [[fase-00-fundacao]] | — | [~] falta o teste em duas máquinas físicas |
| F01 | [[fase-01-nucleo-do-cluster]] | F00 | [x] |
| F02 | [[fase-02-capability-e-saude]] | F01 | [~] |
| F03 | [[fase-03-execucao-distribuida]] | F01, F02 | [x] |
| F04 | [[fase-04-escalonamento]] | F02, F03 | [x] |
| F05 | [[fase-05-observabilidade]] | F03, F04 | [x] |
| F06 | [[fase-06-estado-e-recuperacao]] | F03, F05 | [x] |
| F07 | [[fase-07-seguranca-base]] | F01, F06 | [x] |
| F08 | [[fase-08-inferencia]] | F03, F05, F06, F07 | [~] |
| F09 | [[fase-09-transporte]] | F03, F08 | [~] |
| F10 | [[fase-10-inteligencia-de-storage]] | F02, F06 | [~] |
| F11 | [[fase-11-request-manager]] | F08, F04 | [ ] |
| F12 | [[fase-12-model-runtime]] | F08, F09, F10, F11, F06 | [ ] |
| F13 | [[fase-13-graph-engine]] | F05, F06 | [ ] |
| F14 | [[fase-14-edge-optimizer]] | F11, F12, F13 | [ ] |
| F15 | [[fase-15-ai-cluster-os]] | F01, F07, F10 | [ ] |
| F16 | [[fase-16-aceleradores]] | F02, F12, F15 | [ ] |
| F17 | [[fase-17-runtime-adaptativo]] | F05, F12 | [ ] |
| F18 | [[fase-18-worker-movel]] | F09, F15, F16 | [ ] |
| F19 | [[fase-19-hardening-de-producao]] | F07, F15 | [ ] |

## Cadeia de dependências

```text
F00 → F01 → F02 → F03 → F04 → F05 → F06 → F07 → F08 → F09 ─┐
                                          F06 → F10 ────────┤→ F12 → F14 → F17
                                  F08 → F11 ────────────────┘     ↑
                                  F05/F06 → F13 ──────────────────┘
                     F01/F07/F10 → F15 → F16 → F18        F07/F15 → F19
```

A ordem é por **dependência**, não cronológica: a inferência (F08) fica depois de segurança (F07) porque o canal de fatias exige a credencial por nó e o papel de inferência; o AI Cluster OS (F15) fica tarde porque nada antes do worker móvel nem do hardening depende dele.

## Gate padrão entre fases

Uma fase só é dada como entregue quando tem: **código** testado, **benchmark** (quando afeta desempenho), **telemetria** que explica o comportamento, **teste de falha** do que pode quebrar, **documentação** atualizada e **ADR** quando houve decisão de arquitetura. Cada nota de fase lista o gate específico.

## Organização

- Uma pasta por fase, `NN-nome/`, com a nota `fase-NN-nome` (que também é o índice da pasta) e os slices `fNN-*`.
- Todo slice tem `superado:` no frontmatter e o checkbox `- [ ] Superado` abaixo do título; a regra está em [[convencoes]].
- Transversal: [[transversal]] ([[marcos]], [[estrategia-de-testes]], [[evidencias-e2e]]).

> [!warning] Colisão de nomes antigos
> Nos documentos antigos, a letra **J** significava duas coisas: *AI Cluster OS* no plano de execução (hoje [[fase-15-ai-cluster-os]]) e *inferência* nos docs de implementação (J1–J5, hoje [[fase-08-inferencia]]). A letra **M** também aparecia como fase *Edge Optimizer* e como marcos M0–M7. Os nomes novos eliminam a ambiguidade; veja [[equivalencia-de-fases]].
