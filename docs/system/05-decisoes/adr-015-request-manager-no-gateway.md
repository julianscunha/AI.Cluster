---
title: "ADR-015 — Request Manager no gateway"
aliases: ["ADR-015", "Request Manager"]
tags: [sistema/adr, subsistema/ia]
type: sistema/adr
---

# ADR-015 — Request Manager no gateway

Status: decidido · Data: 2026-10-25 · Voltar para [[decisoes]].

## Contexto

O gateway aceitava uma chave só, sem cota nem ordem de atendimento, e respondia 429 a quem chegasse com a geração ocupada. O controller não sabia de quem era uma requisição nem se alguém ainda a esperava. O alvo ([[request-manager]]) pede identidade, admissão, fila, cota, prioridade e cancelamento.

## Decisão

1. **Primeiro passo dentro do gateway.** Identidade, ACL, cota e fila vivem em `request_manager.rs`, no processo que já recebe a requisição e já conta os tokens. Um serviço separado fica para quando houver mais de um gateway ou estado compartilhado a manter.
2. **Tenant é uma chave com política.** Cada chave tem modelos permitidos, cota de tokens por janela e uma classe máxima. A chave única de antes continua sendo o tenant `default` sem limites, para não quebrar quem já usa.
3. **Cota na entrada, conta na saída.** A cota é conferida antes de a requisição entrar na fila e de novo ao sair dela; os tokens são cobrados ao fim da geração, quando o número real é conhecido. O preço é que a última requisição da janela pode passar do limite pelo próprio tamanho.
4. **Fila por classe com envelhecimento.** A geração é um recurso exclusivo, entregue à maior classe e, dentro dela, ao mais antigo; cada 5 s de espera vale uma classe. A classe pedida nunca passa do teto do tenant. Quem sai da fila (timeout ou cliente que desconectou) é removido por um guard, e se o slot já era seu ele o repassa.
5. **Prioridade chega ao controller.** A classe vira o `priority` das unidades do grafo ([[adr-011-escalonamento-por-custo]]), então a ordem da fila do gateway e a da fila do controller concordam.
6. **Cancelar é um RPC do controller.** `CancelRequest` tira da fila as unidades de uma requisição que ninguém espera mais. O gateway o dispara por um guard no `Drop`, o que cobre prazo, erro e cliente que saiu. O que já está num worker termina.

## Consequências

- Um cliente que esgota a cota recebe 429 com `Retry-After`; os outros tenants não sentem.
- A fila só ordena o chat; os embeddings passam por um semáforo de capacidade.
- Cada tenant só enxerga os próprios contadores em `/metrics`.

## Alternativas

- Serviço Request Manager separado agora: mais uma peça e um protocolo antes de existir o segundo gateway que o justifique.
- Fila FIFO simples: não deixa uma requisição interativa passar à frente de um lote.
- Prioridade estrita sem envelhecimento: uma corrente de requisições altas mataria as baixas de fome (a mesma escolha do ADR-011).
- Cobrar a cota antes de gerar por uma estimativa (`prompt + max_tokens`): evita o excesso mas cobra por tokens que a geração talvez não produza.

## Limites

- Cota, fila e contadores ficam na memória do gateway: somem no restart e não somam entre gateways.
- Chaves ficam em variável de ambiente, sem rotação.
- Planejado: estados de requisição, `Idempotency-Key`, equidade entre tenants, orçamento de recursos, cota persistida e cancelamento de unidade em execução ([[request-manager]]).
