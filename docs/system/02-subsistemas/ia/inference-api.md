---
title: "Inference API"
aliases: ["API de inferência", "docs/23", "23 — Inference API", "AI Cluster — Inference API"]
tags: [sistema/subsistema/ia]
type: sistema/subsistema
status: decidido
implementacao: parcial
fases: ["[[fase-08-inferencia]]"]
---

# Inference API

A Inference API é a fronteira entre o **Application Plane** (agentes, orquestradores, aplicações, CLIs) e o **AI Compute Plane**. Todo consumo de capacidade de IA passa por ela; nenhum cliente fala com o controller, com workers, shards, tensores ou estado interno. Ver [[adr-007-fronteira-inference-api]]. Voltar para [[ia]].

## Objetivo

Expor o cluster como um endpoint de inferência estável, enquanto a topologia interna evolui. Não expõe como a requisição foi distribuída.

Não devem receber acesso direto ao Data Plane: Worker Runtime, Model Manager, State Manager, armazenamento interno e interfaces administrativas dos workers.

## Separação de planos

```text
Application Plane
  | HTTPS / API
  v
Inference API ........... autenticação, validação, normalização, streaming
  v
Request Manager ......... admissão, fila, quotas, orçamento (ver request-manager)
  +--> Edge Optimizer, Model Manager, Execution Planner, Scheduler, State Manager
  v
Distributed Inference
```

Administração não usa esta API: nós, planos, workers, métricas, traces, provisionamento e recuperação ficam na Management API / CLI ([[operacao]]).

## Contrato público

### Compatibilidade

A primeira interface deve ser compatível com o padrão de APIs de inferência usado pelos clientes de LLM, desde que não comprometa a semântica do runtime. A compatibilidade é uma camada de adaptação: o protocolo interno do cluster não segue o externo.

```text
POST /v1/chat/completions
POST /v1/responses
GET  /v1/models
POST /v1/embeddings
Futuro: POST /v1/rerank, POST /v1/transcriptions, POST /v1/generate
```

### Objeto de requisição

Toda requisição recebe um identificador global, usado em logs, métricas, traces, estado, grafo e auditoria.

```json
{
  "request_id": "req-1842",
  "model": "llama-x-3.2-q4",
  "priority": "interactive",
  "stream": true,
  "input_tokens_estimate": 2481,
  "output_token_limit": 4096
}
```

Uma request pode ter várias execuções (retry, recovery). A resposta representa a request; a observabilidade guarda o histórico de cada `execution_id`.

### Streaming

Streaming é requisito de primeira classe. O consumidor não precisa saber em quais nós os tokens foram produzidos. Metadados de uso vão de forma compatível, sem expor métricas internas confidenciais na resposta padrão.

### Cancelamento

O cliente pode cancelar uma execução. Fluxo interno: API → Request Manager → Execution Manager → cancelar Work Units pendentes → parar a geração → liberar reservas e estado → emitir evento. O cancelamento é idempotente.

### Timeout, deadline e limites

A request pode trazer `request timeout`, `execution deadline` e `max output tokens`. Esses valores alimentam o Scheduler, mas nunca violam garantias de integridade de estado.

### Prioridades

Classes controladas por política: `realtime`, `interactive`, `normal`, `batch`, `background`. O cliente não define prioridade crítica sem autorização.

### Contabilização de tokens

Por request, o cluster registra `input_tokens_received`, `input_tokens_after_optimization`, `cached_tokens`, `output_tokens_generated`, `tokens_avoided` e `tokens_total_processed`.

### Latência

Correlacionáveis: latência total, TTFT, latência entre tokens, throughput de geração, latência de borda, de planejamento e de fila.

### Modelo de erro

Categorias estáveis: `INVALID_REQUEST`, `AUTHENTICATION_FAILED`, `AUTHORIZATION_FAILED`, `MODEL_NOT_FOUND`, `MODEL_UNAVAILABLE`, `CAPACITY_UNAVAILABLE`, `TIMEOUT`, `CANCELLED`, `INTERNAL_ERROR`, `SERVICE_UNAVAILABLE`. Detalhes internos ficam em trace, evento e log.

### Descoberta de modelo

`GET /v1/models` expõe só modelos e variantes autorizados ao consumidor. Existência física de shards e nós não faz parte do contrato.

### Política de roteamento

O cliente pode pedir `model`, `quality target` e `latency target`. Não escolhe `node`, `shard`, `worker`, `transport` nem `placement`.

### Headers de observabilidade

Quando couber: `request_id`, `trace_id`, `model_id`, `model_version`, `usage`.

## Segurança e isolamento

- Externa: TLS, autenticação, autorização, rate limiting, limite de tamanho de requisição, ACL de modelo e cota.
- Acesso termina na API; endpoints de worker nunca são expostos.
- Mesmo em deployment single-tenant, o modelo separa identidade por `tenant`, `application`, `API key`, quota, ACL de modelo e política de prioridade, para evitar migração futura.

Papéis de credencial (join, operador, inferência) e a chave de API são tratados em [[seguranca]].

## Responsabilidades

| API | Cluster interno |
|---|---|
| Receber, autenticar, autorizar, validar, normalizar | Otimização de contexto e retrieval ([[edge-optimizer]], [[graph-engine]]) |
| Iniciar e cancelar execução | Seleção de modelo ([[model-manager]]) e planejamento ([[execution-planner]]) |
| Transmitir resposta | Escalonamento ([[scheduler]]), estado ([[state-manager]]), transporte e execução |
| Expor uso e identificadores | Execução distribuída ([[inference-engine]]) |

## Saúde

A saúde da API é separada da do cluster (ex.: API verde, cluster amarelo, capacidade 72%). O endpoint de health não executa inferência real a cada chamada.

## Backpressure e fallback

Sem capacidade, a API aplica política explícita: `queue`, `reject`, `retry-after` ou `fallback model`. O cliente não deve abrir centenas de requisições em paralelo para contornar a capacidade.

Fallback interno pode trocar de variante ou de placement. Fallback de modelo só ocorre com política explícita, porque altera qualidade e comportamento.

> [!info] O que já existe (MVP, fatias F08)
> - Crate `code/crates/gateway` (binário `ai-cluster-gateway`). Fala com o controller como **operador** (`AI_CLUSTER_OPERATOR_TOKEN`) e herda o TLS de cliente (`AI_CLUSTER_TLS_CA`).
> - Rotas: `GET /health` (sem chave, não toca o cluster); `GET /v1/models`; `POST /v1/embeddings` sobre o modelo sintético `matmul-diamond-v0` (grafo diamante 16×16 executado de verdade nos workers; vetor sem semântica, resposta com `"synthetic": true`); `POST /v1/chat/completions` sobre GPT-2 small e SmolLM2-135M (ids `gpt2`, `smollm2-135m` e `smollm2-135m-instruct`).
> - Autenticação: `Authorization: Bearer` com `AI_CLUSTER_API_KEY`, comparada sem saída antecipada. A chave é obrigatória e diferente dos tokens de join, operador e inferência; o gateway não sobe se coincidirem.
> - Limites: corpo máximo de 64 KiB; `input` de 1 a 8192 bytes; embeddings com 4 pedidos em andamento; chat com uma geração por vez (`llm_slot`), prompt até 512 tokens e 8 KiB, até 32 mensagens, `max_tokens` 1–64 (ajustado, não rejeitado), prazo de 30 s. Acima dos tetos: 429 com `Retry-After`.
> - Erros emitidos: `AUTHENTICATION_FAILED` (401), `MODEL_NOT_FOUND` (404), `INVALID_REQUEST` (400/413), `CAPACITY_UNAVAILABLE` (429), `TIMEOUT` (504), `MODEL_UNAVAILABLE` (503), `SERVICE_UNAVAILABLE` (503). A causa interna vai para o log.
> - Escuta em `127.0.0.1:8080`; outro endereço só com `AI_CLUSTER_INSECURE=1`.
> - Correlação: cabeçalhos `x-request-id` e `x-model-id`; o `request_id` também vai no corpo e leva a `ai-cluster request trace <id>`.
> - Chat com `stream: true`: `text/event-stream` no formato OpenAI; erro depois dos cabeçalhos chega como evento `{"error": ...}` seguido de `[DONE]`; cliente que sai cancela no próximo token. Sem `usage` no stream.
> - Modelo base (GPT-2, SmolLM2) só continua texto (mensagens unidas por quebra de linha); instruct recebe ChatML com tokens especiais por id, e texto do usuário com `<|im_end|>` não fecha turno.
> - Controller: `EnqueueGraph` com `keep_result` e RPC `GetOutput` (só operador), guardando as últimas 64 saídas; fila de no máximo 256 unidades (`ResourceExhausted`, 429 no gateway).
> - Evidência: `code/tests/phase-j-inference.sh`, `code/tests/phase-j-chat.sh [gpt2|smollm2|instruct]`, `code/crates/engine/tests/smollm2_chat_parity.rs`.
>
> Slices: [[f08-gateway-e-api]], [[f08-chat-completions]].

> [!warning] Desvios do alvo e migração
> 1. **Sem fronteira entre API e motor.** O modelo roda dentro do gateway (cerca de 1,6 GB de RAM), que depende do crate `engine`. Só as projeções vão a workers, quando `AI_CLUSTER_*_SHARDS` está definido. Migração: [[fase-12-model-runtime]] (motor como serviço fora do gateway).
> 2. **Sem Request Manager.** Não há admissão por orçamento, fila por classe, quota nem prioridade. O controle de entrada é um semáforo (4 embeddings; 1 geração de chat) que responde 429. Migração: [[fase-11-request-manager]].
> 3. **Nós escolhidos por variável de ambiente.** `AI_CLUSTER_GPT2_SHARDS` e `AI_CLUSTER_SMOLLM2_SHARDS` listam os nós que recebem faixas; não há Planner nem Scheduler no caminho de inferência. Migração: [[fase-12-model-runtime]] com [[execution-planner]] e [[scheduler]].
> 4. **Sem versão de modelo.** `model_version` não é retornado; os pesos são lidos do disco de cada worker, por caminho fixo (`model.safetensors`), sem checksum. Migração: registry e manifest em [[model-manager]].
> 5. **Uma chave só.** Não há tenant, cota, ACL nem prioridade; `GET /v1/models` lista todos os modelos carregados.
> 6. **Sem endpoint de cancelamento.** Geração de chat cancela ao desconectar ou estourar o prazo; grafo de embeddings já enfileirado continua na fila do controller e roda quando um worker aparece.
> 7. **Perna HTTP sem TLS.** Fora de loopback (com `AI_CLUSTER_INSECURE=1`), a chave trafega em claro.
> 8. **Endpoints e campos ausentes.** `/v1/responses` não existe; `/v1/embeddings` é sintético; `temperature`, `n` e `stop` são ignorados (geração gulosa, uma escolha); não há `usage` no stream.
> 9. **Erros ausentes.** `CANCELLED`, `AUTHORIZATION_FAILED` e `INTERNAL_ERROR` não são emitidos.
> 10. **Sem contabilização de tokens** (`tokens_avoided`, tokens após otimização) e sem `Idempotency-Key`.

> [!todo] Falta
> - Request Manager (admissão, fila por classe, quotas, orçamento, prioridades): [[request-manager]], [[fase-11-request-manager]].
> - Edge Optimizer antes da alocação pesada: [[edge-optimizer]], [[fase-14-edge-optimizer]].
> - Tenants, quotas, ACL de modelo e autorização de prioridade.
> - Cancelamento por `request_id` (`DELETE`) que propaga até as Work Units.
> - Contabilização de tokens e `usage` no stream.
> - `/v1/responses`, `/v1/rerank`, `/v1/transcriptions`.
> - TLS e autenticação mútua na perna de aplicações (ver [[seguranca]]).
