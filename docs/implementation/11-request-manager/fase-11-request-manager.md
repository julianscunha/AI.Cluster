---
title: "F11 — Request Manager"
aliases: ["docs/24", "24 — Request Manager", "Request Manager", "Tetos de F08"]
tags: [implementacao/fase/11, implementacao/estado/planejado]
type: implementacao/fase
status: planejado
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

- [ ] Request Manager: estados da request, admissão e fila. Não existe. O gateway usa contagem fixa.
- [~] Admissão ad hoc, com tetos fixos: 4 pedidos em andamento no gateway (429 com `Retry-After`); 1 geração de chat por vez (429); 256 unidades na fila do controller (`ResourceExhausted` vira 429 com `Retry-After: 5`); 8 `ShardMatmul` simultâneos; 64 operações em voo por nó.
- [ ] Quotas e orçamento de tokens e de recursos. Não existem.
- [ ] Prioridades e herança de prioridade. Não existem (nenhum campo de prioridade no proto).
- [ ] Idempotência, equidade e backpressure do spec (§12, §21, §22).

Cancelamento e deadlines:

- [x] Chat local: a flag de cancelamento é checada a cada token. Vale para prazo de 30 s e para cliente que sai.
- [~] `ShardMatmul` em voo: o controller esquece as operações quando a chamada termina. O worker não recebe sinal de cancelamento.
- [ ] Grafo enfileirado no controller: `EnqueueGraph` não tem cancelamento. Um pedido que estoura o prazo deixa o grafo na fila, e os grafos rodam quando um worker aparece.
- [ ] Unidades em execução no worker: o worker não tem caminho de cancelamento.
- [~] Deadlines: prazo fixo de 30 s no gateway e de 8 s no `ShardMatmul`. Não há deadline por requisição.

Tetos herdados de F08:

- [ ] Entrada arbitrária no grafo. Hoje só o grafo sintético `matmul-diamond-v0` e os modelos fixos.
- [ ] Uma chave só: não há tenant, cota nem ACL. Qualquer portador usa o modelo.
- [ ] Multi-tenant e ACL de modelo.
- [ ] Contagem de tokens por cliente.
- [ ] TLS na perna HTTP do gateway. Hoje escuta em `127.0.0.1` e só aceita outro endereço com `AI_CLUSTER_INSECURE=1`. Também aparece em [[fase-19-hardening-de-producao]].
- [x] `/v1/chat/completions` e streaming (já entregues em J3, fora da lista de tetos).
- [~] Resultado por `request_id`: o `GetOutput` é só de operador e o gateway devolve só o resultado do pedido que ele criou.

> [!warning] Tetos atuais
> - Sem cancelamento até o worker, pedidos que estouram o prazo consomem fila e CPU sem ninguém esperando.
> - Uma chave só e sem cota: um cliente pode esgotar o cluster.
> - Os limites de admissão são constantes no código, não políticas.

## Gate

Gate padrão (EP §19): código, benchmarks, telemetria, testes de falha, atualização de documentação e ADR quando a arquitetura mudar.

Gate específico (manifesto): quotas e fila funcionando sob carga.

Testes de falha exigidos pela absorção: cliente que sai com grafo na fila faz as unidades não iniciadas saírem da fila; cota estourada responde 429 sem derrubar outros tenants.

## Slices

Notas desta pasta (esta nota é o MOC): nenhum slice criado ainda.

Slices a criar:

- `f11-admissao-e-fila`: estados da request, admissão e fila (Request Manager, §1–§6).
- `f11-cancelamento-ate-o-worker`: cancelamento de grafo enfileirado e de unidades em execução.
- `f11-quotas-e-orcamento`: quotas, orçamento de tokens e de recursos, contagem de tokens.
- `f11-prioridades-e-deadlines`: prioridades, herança e deadline por requisição.
- `f11-tenants-e-acl`: chave por tenant, ACL por modelo e multi-tenant.
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
