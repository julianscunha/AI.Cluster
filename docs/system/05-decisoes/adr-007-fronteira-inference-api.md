---
title: "ADR-007 — API como fronteira entre Application Plane e AI Cluster"
aliases: ["ADR-007", "ADR-007 — API como fronteira entre Application Plane e AI Cluster"]
tags: [sistema/adr, subsistema/ia]
type: sistema/adr
status: decidido
implementacao: parcial
fases: ["[[fase-08-inferencia]]"]
---

# ADR-007 — API como fronteira entre Application Plane e AI Cluster

Data: 2026-10-19 · Voltar para [[decisoes]].

## Contexto

O AI Cluster é uma infraestrutura especializada em otimização e execução de cargas de IA. Agentes, aplicações e orquestradores têm outras responsabilidades: interação com usuário, uso de ferramentas, workflows e lógica de negócio.

Expor o Data Plane diretamente faria o cluster acumular responsabilidades fora da sua especialidade e criaria acoplamento com cada agente.

## Decisão

O AI Cluster é consumido por meio de uma **Inference API** ([[inference-api]]). Nenhum cliente externo acessa diretamente Controller, Worker, Model Shard, Tensor ou State Store.

Agentes como Hermes e Codex, aplicações próprias e orquestradores consomem o cluster pela API.

A interface inicial busca compatibilidade com APIs de inferência de LLM amplamente usadas, sem obrigar o protocolo interno a seguir a mesma representação.

## Consequências

Positivas:

- desacoplamento entre Application Plane e Compute Plane;
- menor consumo de recursos do cluster;
- liberdade para mudar topologia e scheduler;
- integração com múltiplos agentes e aplicações;
- segurança e autorização centralizadas;
- observabilidade por request.

Negativas:

- uma camada adicional de API;
- necessidade de manter compatibilidade do contrato externo;
- possível overhead de serialização no gateway.

## Regra

A Inference API é a única fronteira suportada para cargas externas. Interfaces de administração e protocolos internos ficam separados.

## Alternativas

- Expor o Data Plane diretamente: descartado pelo acoplamento descrito no contexto.

> [!warning] Desvio do MVP
> O gateway hoje roda o modelo dentro do próprio processo (GPT-2 e SmolLM2, com as projeções opcionalmente nos workers) e não há Request Manager. A fronteira é respeitada para clientes, que só falam HTTP com a chave de API, mas o gateway ainda acumula o papel de engine. A migração prevista está em [[request-manager]] e [[inference-engine]].

> [!info] O que já existe
> Gateway HTTP com `/v1/models`, `/v1/embeddings` e `/v1/chat/completions` (streaming incluso), autenticado por chave de API separada dos tokens do cluster ([[f08-gateway-e-api]], [[fase-08-inferencia]]). Ver [[adr-010-papeis-de-credencial]].

> [!todo] Request Manager (admissão, filas, cotas, prioridades) e ACL por modelo ficam em [[fase-11-request-manager]].
