---
title: "Decisões"
aliases: ["ADRs", "Decisões de arquitetura", "05-decisoes"]
tags: [sistema/moc, adr]
type: sistema/moc
status: vivo
implementacao: parcial
---

# Decisões

Registros de decisão de arquitetura (ADR) do sistema. Cada ADR tem contexto, decisão, consequências e alternativas, e liga-se à fase que a entrega. Voltar para [[system]] ou [[inicio]].

| ADR | Status | Data | Decisão |
|---|---|---|---|
| [[adr-001-base-linux]] | proposto | 2026-10-19 | Construir sobre uma base Linux madura, sem kernel próprio |
| [[adr-002-os-imutavel]] | proposto | 2026-10-19 | Imagem versionada e imutável, com reimage e update A/B |
| [[adr-003-controller-worker]] | decidido | 2026-10-19 | Separar control plane (controller) e Worker Runtime |
| [[adr-004-grafo-como-projecao]] | proposto | 2026-10-19 | Grafo como projeção de relações, não como fonte única de verdade |
| [[adr-005-camadas-de-storage]] | decidido | 2026-10-19 | RAM e VRAM como HOT, SSD e NVMe como WARM, HDD como COLD |
| [[adr-006-hotplug]] | decidido | 2026-10-19 | Hot-plug por state objects, checkpoints e replanejamento, sem migrar memória viva |
| [[adr-007-fronteira-inference-api]] | decidido | 2026-10-19 | A Inference API é a única fronteira para cargas externas |
| [[adr-008-stack-inicial]] | vivo | 2026-10-19 | Rust 2024, tokio, gRPC e Protobuf, tonic 0.14, axum 0.8 e clap 4 |
| [[adr-009-canal-residente-de-fatias]] | decidido | 2026-10-24 | Fatiar projeções com pesos residentes nos workers e canal bidirecional pelo controller |
| [[adr-010-papeis-de-credencial]] | decidido | 2026-10-23 a 10-24 | Join, operador, inferência, credencial por nó e chave de API, cada um com segredo próprio e falha fechada |
| [[adr-011-escalonamento-por-custo]] | decidido | 2026-10-25 | O nó pede, o controller decide por custo com histerese; prioridade com envelhecimento, slots no worker e quarentena por evidência |
| [[adr-013-estado-do-controller-e-recuperacao]] | decidido | 2026-10-25 | Snapshot atômico do conjunto vivo, posse preguiçosa, dois relógios de recuperação, cópia num nó e eventos em arquivo |
| [[adr-012-metricas-de-inferencia-no-gateway]] | decidido | 2026-10-25 | O gateway mede TTFT, ITL, tokens/s e KV cache de cada geração e os expõe em `GET /metrics` |

## Notas

- Status `proposto` ainda não tem implementação; `decidido` vale para o alvo, mesmo que parcial; `vivo` é revisado com o código.
- ADR-001 a ADR-008 foram registrados juntos em 2026-10-19; a data é a do primeiro commit que os trouxe ao repositório.
- ADR-009 e ADR-010 registram decisões tomadas durante a implementação da Fase J5 e da Fase S; os textos seguem o que o código faz hoje.
