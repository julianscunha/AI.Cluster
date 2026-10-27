---
title: "ADR-007 — API como fronteira entre Application Plane e AI Cluster"
aliases: ["ADR-007", "ADR-007 — API como fronteira entre Application Plane e AI Cluster"]
tags: [sistema/adr, subsistema/ia]
type: sistema/adr
---

# ADR-007 — API como fronteira entre Application Plane e AI Cluster

Status: decidido · Data: 2026-10-19 · Voltar para [[decisoes]].

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

Hoje a API é um gateway HTTP com `/v1/models`, `/v1/embeddings` e `/v1/chat/completions` (com streaming SSE), autenticado por uma chave de API separada dos tokens do cluster ([[adr-010-papeis-de-credencial]]).

## Regra

A Inference API é a única fronteira suportada para cargas externas. Interfaces de administração e protocolos internos ficam separados.

## Alternativas

- Expor o Data Plane diretamente: descartado pelo acoplamento descrito no contexto.

## Limites

- O gateway ainda executa o modelo dentro do próprio processo (GPT-2 e SmolLM2; só as projeções podem ir para os workers). Ele acumula, portanto, também o papel de engine. Clientes só falam HTTP com a chave de API.
- A admissão, a cota e a fila estão no gateway ([[adr-015-request-manager-no-gateway]]). Planejado: separar a engine do gateway ([[inference-engine]]).
