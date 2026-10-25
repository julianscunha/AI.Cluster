---
title: "F11 — Request Manager"
aliases: ["docs/24", "24 — Request Manager", "Request Manager", "Tetos de F08"]
tags: [implementacao/fase/11, implementacao/estado/parcial]
type: implementacao/fase
status: parcial
codigos-antigos: ["docs/24", "tetos de f08-gateway-e-api (Tetos e o que vem depois)", "pendências da auditoria: Request Manager, cancelamento até o worker, quotas"]
sistema: ["[[request-manager]]", "[[inference-api]]", "[[seguranca]]", "[[scheduler]]"]
---
# F11 — Request Manager

## Objetivo

Controlar as requisições de inferência antes de elas chegarem ao cluster: admissão, fila, quotas, orçamento de tokens e de recursos, prioridades e deadlines, com cancelamento que chega até o worker e identidade por tenant. Hoje o gateway limita por contagem fixa e não há nada entre a API e o controller.

## Depende de

- [[fase-08-inferencia]]: gateway, API e os tetos listados em `f08-gateway-e-api`.
- [[fase-04-escalonamento]]: admissão e placement; a fila de requisições fica separada do escalonador (spec §24).

## Entrega

- Request Manager conforme o spec [[request-manager]]: estados da request, controle de admissão, fila, quotas, orçamento de tokens e de recursos, rejeição antecipada, backpressure, cancelamento, deadlines, prioridades, idempotência e equidade.
- Cancelamento até o worker: grafo enfileirado e unidades em execução.
- Multi-tenant: chave por tenant, ACL por modelo, cotas e contagem de tokens.
- Absorção dos tetos de F08 (lista abaixo).

## Checklist

Verificado em `code/crates/gateway/src/main.rs`, `code/crates/controller/src/service.rs` e `shards.rs`.

Request Manager:

- [~] Request Manager: identidade, cota e fila por classe no gateway ([[f11-tenants-quotas-e-fila]]); estados da request e idempotência não existem.
- [~] Admissão ad hoc, com tetos fixos: 4 pedidos em andamento no gateway (429 com `Retry-After`); 1 geração de chat por vez (429); 256 unidades na fila do controller (`ResourceExhausted` vira 429 com `Retry-After: 5`); 8 `ShardMatmul` simultâneos; 64 operações em voo por nó.
- [~] Quotas de tokens por tenant e janela (`tokens=N`); orçamento de recursos não existe.
- [x] Prioridades: classes `realtime`..`background` em `x-priority`, capadas pelo teto do tenant, ordenam a fila do gateway e viram `priority` das unidades no controller (F04). Herança além disso não existe.
- [ ] Idempotência, equidade e backpressure do spec (§12, §21, §22).

Cancelamento e deadlines:

- [x] Chat local: a flag de cancelamento é checada a cada token. Vale para prazo de 30 s e para cliente que sai.
- [~] `ShardMatmul` em voo: o controller esquece as operações quando a chamada termina. O worker não recebe sinal de cancelamento.
- [x] Grafo enfileirado no controller: `CancelRequest` tira da fila o que não começou; o gateway o chama quando o pedido estoura o prazo (`phase-m-request-manager.sh`).
- [ ] Unidades em execução no worker: o worker não tem caminho de cancelamento.
- [~] Deadlines: prazo fixo de 30 s no gateway e de 8 s no `ShardMatmul`. Não há deadline por requisição.

Tetos herdados de F08:

- [ ] Entrada arbitrária no grafo. Hoje só o grafo sintético `matmul-diamond-v0` e os modelos fixos.
- [x] Uma chave só: tenants com chave própria (`AI_CLUSTER_TENANTS`), ACL de modelo e contagem de tokens por tenant.
- [ ] TLS na perna HTTP do gateway. Hoje escuta em `127.0.0.1` e só aceita outro endereço com `AI_CLUSTER_INSECURE=1`. Também aparece em [[fase-19-hardening-de-producao]].
- [x] `/v1/chat/completions` e streaming (já entregues em J3, fora da lista de tetos).
- [~] Resultado por `request_id`: o `GetOutput` é só de operador e o gateway devolve só o resultado do pedido que ele criou.

> [!warning] Tetos atuais
> - Unidade já em execução no worker não é cancelada: pedidos que estouram o prazo ainda gastam CPU até o fim da unidade.
> - Cota e fila vivem na memória do gateway: somem no restart e não valem entre vários gateways.
> - A fila por classe cobre só a geração de chat; os embeddings ainda usam o semáforo.

## Gate

Gate padrão (EP §19): código, benchmarks, telemetria, testes de falha, atualização de documentação e ADR quando a arquitetura mudar. ADR: [[adr-015-request-manager-no-gateway]].

Gate específico (manifesto): quotas e fila funcionando sob carga.

Testes de falha exigidos pela absorção: cliente que sai com grafo na fila faz as unidades não iniciadas saírem da fila; cota estourada responde 429 sem derrubar outros tenants.

## Slices

Notas desta pasta (esta nota é o MOC):

- [[f11-tenants-quotas-e-fila]]: tenants, ACL de modelo, cota de tokens, fila por classe e cancelamento do grafo.

Slices a criar:

- `f11-estados-e-idempotencia`: estados da request, `Idempotency-Key`, equidade entre tenants.
- `f11-cancelamento-ate-o-worker`: cancelamento de unidades em execução e de `ShardMatmul` em voo.
- `f11-entrada-arbitraria-no-grafo`: API para grafo de entrada arbitrária.

## Códigos antigos

- `docs/24` (Request Manager) → [[request-manager]].
- Tetos de [[f08-gateway-e-api]] (seção "Tetos e o que vem depois").
- Pendências abertas da auditoria: Request Manager, cancelamento até o worker, quotas.

## Specs do sistema

- [[request-manager]]: spec-alvo do Request Manager (estados, admissão, fila, quotas, cancelamento, deadlines).
- [[inference-api]]: contrato da API que este gerenciador protege.
- [[seguranca]]: papéis e chaves de API.
- [[scheduler]]: fronteira entre fila de requisições e escalonamento.
