---
title: "F08 — Inferência"
aliases: ["Fase J", "fase-j", "Inference API", "J1–J5", "ADR-007 implementação"]
tags: [implementacao/fase/08, implementacao/estado/parcial]
type: implementacao/fase
status: parcial
codigos-antigos: ["J1", "J2", "J3", "J4", "J5", "ADR-007"]
sistema: ["[[inference-api]]", "[[inference-engine]]", "[[request-manager]]", "[[model-manager]]", "[[canal-de-fatias]]"]
---
# F08 — Inferência

Esta é a nota da fase e o MOC de `08-inferencia/`.

> [!warning] Colisão do rótulo "J"
> No plano mestre (EP), "J" é a Fase J do **AI Cluster OS** (imagem, bootstrap, reimage), que
> corresponde à F15 (`codigos-antigos` J1–J6 nela). Aqui "J1" a "J5" são as fatias de **inferência**.
> Quando um documento cita "J" sem contexto, confira a fase.

## Objetivo

Aplicações pedem inferência por uma API HTTP; o modelo roda no cluster, com resultado idêntico ao
local e com um canal que sobrevive à falha de um worker. Critério de gate: logits idênticos entre
local e fatiado; 503 sem pendurar quando um worker cai.

## Depende de

- [[fase-03-execucao-distribuida]] (Work Unit e DAG para o caminho de embeddings)
- [[fase-05-observabilidade]] (trace do pedido, `ai-cluster request trace`)
- [[fase-06-estado-e-recuperacao]] (reatribuição e recuperação)
- [[fase-07-seguranca-base]] (chaves e papéis separados, canal autenticado)

## Entrega

- Gateway HTTP com chave de API, modelos listados, embeddings sintéticos e chat com streaming.
- Engine CPU de GPT-2 e SmolLM2 (base e instruct) com paridade contra `transformers`.
- Projeções de camada nos workers, em fatias residentes, com reatribuição de faixas em falha.
- Papel de inferência separado do operador.

## Checklist

| Estado | Item | Slice |
| --- | --- | --- |
| [x] | J1 gateway HTTP da API sobre modelo sintético (`matmul-diamond-v0`) | [[f08-gateway-e-api]] |
| [x] | J2 forward do GPT-2 small em CPU, KV cache e paridade | [[f08-engine-local]] |
| [x] | J3 `/v1/chat/completions` sobre GPT-2 no gateway | [[f08-chat-completions]] |
| [x] | J3 streaming SSE no formato OpenAI, com cancelamento ao desconectar | [[f08-chat-completions]] |
| [x] | J4 SmolLM2-135M (Llama) com paridade, base e instruct com template ChatML | [[f08-engine-local]], [[f08-chat-completions]] |
| [x] | J5 canal residente de fatias, GPT-2 e SmolLM2 fatiados, por linhas e colunas | [[f08-canal-residente-de-fatias]] |
| [x] | J5 tolerância a falha de worker por reatribuição de faixas | [[f08-canal-residente-de-fatias]] |
| [x] | J5 papel de inferência separado (`AI_CLUSTER_INFERENCE_TOKEN`) | [[f08-canal-residente-de-fatias]] |
| [x] | J5 prazo, cancelamento, allowlist de projeções, cache LRU e flag `invalid` | [[f08-canal-residente-de-fatias]] |
| [~] | Gateway concentra o modelo e escolhe nós por variável de ambiente | gap, [[fase-12-model-runtime]] |
| [ ] | Request Manager: admissão por orçamento, fila por classe, quotas, prioridades | gap, [[fase-11-request-manager]] |
| [ ] | Pesos com versão, manifest e checksum | gap, [[fase-12-model-runtime]] |
| [ ] | Cancelamento de pedido de embeddings na fila (grafo já enfileirado roda) | gap, [[fase-11-request-manager]] |
| [ ] | Entrada arbitrária no grafo de embeddings (hoje o grafo é o sintético) | gap, [[fase-12-model-runtime]] |
| [ ] | Estado KV por chave no cluster (hoje o KV cache é do gateway) | gap, [[fase-12-model-runtime]] |
| [ ] | Pipeline, tensor e híbrido; all-reduce | gap, [[fase-12-model-runtime]] |
| [ ] | Amostragem com seed; `temperature`, `top_p`, `stop` | gap, [[fase-12-model-runtime]] |
| [ ] | TLS e autenticação mútua na perna de aplicações | gap, [[fase-19-hardening-de-producao]] |
| [ ] | Contabilização de tokens e `usage` no stream | gap, [[fase-11-request-manager]] |
| [~] | Transporte do canal de fatias só gRPC/TCP | gap, [[fase-09-transporte]] |

Legenda: `[x]` implementado e validado · `[~]` parcial · `[ ]` não iniciado.

## Gap: arquitetura-alvo × MVP

A arquitetura-alvo da [[inference-api]] separa três peças que o MVP ainda não tem. O MVP cumpre o
caminho e a paridade, mas com o modelo e a escolha de nós dentro do gateway.

| Peça | Alvo (sistema) | MVP hoje | Fase |
| --- | --- | --- | --- |
| Gateway | só a API: autentica, traduz formato e devolve resposta | roda o modelo (cerca de 1,6 GB) e depende do crate `engine` | [[fase-12-model-runtime]] |
| Placement | controller decide nó e fatia pelo Planner e Scheduler | nós por `AI_CLUSTER_*_SHARDS`; sem Planner nem Scheduler no caminho de inferência | [[fase-12-model-runtime]] com [[execution-planner]] e [[scheduler]] |
| Admissão | [[request-manager]]: orçamento, fila por classe, quota, prioridade, cancelamento | semáforo (4 embeddings; 1 geração de chat) que responde 429 | [[fase-11-request-manager]] |
| Pesos | [[model-manager]]: registry, versão, variante, manifest e checksum | `model.safetensors` lido por caminho fixo em cada worker; sem versão nem checksum | [[fase-12-model-runtime]] |

> [!warning] Desvios do alvo com migração prevista
> 1. **Gateway concentra o modelo e o placement.** Só as projeções vão a workers, e só quando
>    `AI_CLUSTER_*_SHARDS` está definido. Migração: motor como serviço fora do gateway e placement
>    no controller ([[fase-12-model-runtime]]).
> 2. **Sem Request Manager.** Migração: [[fase-11-request-manager]].
> 3. **Pesos sem versão.** `model_version` não é retornado. Migração: registry e manifest em
>    [[model-manager]] ([[fase-12-model-runtime]]).

## Evidência

- `code/tests/phase-j-inference.sh` (gateway e API), `code/tests/phase-j-chat.sh` (chat por modelo),
  `code/tests/phase-j-shards.sh` (canal, failover e papel de inferência).
- Paridade: `code/crates/engine/tests/gpt2_parity.rs`, `smollm2_parity.rs`, `smollm2_chat_parity.rs`
  e `sharded_projections_match_the_local_forward`.
- Custo: `code/crates/engine/examples/bench.rs` (local) e a seção de custo de
  [[f08-canal-residente-de-fatias]].

## Gate

Critério de saída da fase: logits idênticos entre o forward local e o fatiado, e 503 sem pendurar
quando um worker some. **Atendido** para GPT-2 e SmolLM2. O gate padrão do EP §19 tem o seguinte
estado:

| Item | Situação |
| --- | --- |
| Código | feito, em `code/crates/gateway`, `engine`, `controller` e `worker` |
| Benchmark | feito: custo local e custo do canal (3× mais lento que local numa máquina) |
| Telemetria | parcial: trace por `request_id`; não há métricas nem eventos próprios de inferência (por modelo, TTFT, tokens) |
| Teste de falha | feito: worker morto no meio da geração, worker ausente, token de inferência limitado |
| Doc | feito (este conjunto); desvios registrados acima |
| ADR | feito: [[adr-007-fronteira-inference-api]], [[adr-009-canal-residente-de-fatias]] |

## Slices

- [[f08-gateway-e-api]]: J1, gateway, chave de API, erros e modelo sintético.
- [[f08-engine-local]]: J2 e J4 no engine, paridade, KV cache e custo local.
- [[f08-chat-completions]]: J3 e J4 no gateway, streaming SSE e ChatML.
- [[f08-canal-residente-de-fatias]]: J5, canal residente, failover, papel de inferência e limites.

## Códigos antigos

- J1 (gateway), J2 (engine), J3 (chat), J4 (SmolLM2), J5 (canal residente).
- ADR-007 (fronteira entre a API e o motor).
- `docs/23`, `docs/25` e `inference-api.md` / `inference-engine.md` (nomes antigos).

## Specs do sistema

- [[inference-api]]: contrato público, erros, cancelamento e desvios do alvo.
- [[inference-engine]]: motor, modelos, fatias e Model Runtime (alvo).
- [[request-manager]]: admissão, fila e quotas (alvo).
- [[model-manager]]: registry e versão de pesos (alvo).
- [[canal-de-fatias]]: protocolo do canal residente.

> [!info] O que já existe
> Gateway com chave de API e erros estáveis; chat com GPT-2, SmolLM2 base e instruct e streaming;
> engine CPU com paridade; canal residente de fatias com reatribuição e papel de inferência.

> [!todo] O que falta
> Request Manager (F11); placement no controller, pesos versionados, estado KV por chave e modelos
> distribuídos (F12); transporte além de gRPC (F09); TLS e contabilização de tokens (F11, F19).
