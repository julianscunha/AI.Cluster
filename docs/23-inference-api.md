# AI Cluster — Inference API

Status: DECIDED baseline / v0.1

## 1. Objetivo

A Inference API é a fronteira entre o **Application Plane** e o **AI Compute Plane**.

Aplicações, agentes e orquestradores não acessam diretamente Controller, Workers, shards, tensors ou estado interno do cluster. Todo consumo de capacidade de IA ocorre através da API.

Exemplos de consumidores:

- agentes de IA;
- orquestradores;
- aplicações próprias;
- IDE/CLI agents;
- serviços internos.

Exemplos de consumidores que não devem receber acesso direto ao Data Plane:

- Worker Runtime;
- Model Manager;
- State Manager;
- armazenamento interno;
- interfaces administrativas dos workers.

## 2. Separação de planos

```text
Application Plane
  |
  | HTTPS / API
  v
Inference API
  |
  v
Request Manager
  |
  +--> Edge Optimizer
  +--> Graph Engine
  +--> Model Manager
  +--> Execution Planner
  +--> Scheduler
  +--> State Manager
  |
  v
Distributed Inference
```

A API não expõe como a requisição foi distribuída.

## 3. Princípio de compatibilidade

A primeira interface pública deve ser compatível com o padrão de APIs de inferência usado pelos principais clientes de LLM sempre que isso não comprometer a semântica do runtime.

Interface inicial proposta:

```text
POST /v1/chat/completions
POST /v1/responses
GET  /v1/models
POST /v1/embeddings
```

Interfaces futuras podem incluir:

```text
POST /v1/rerank
POST /v1/transcriptions
POST /v1/generate
```

A compatibilidade é uma camada de adaptação. O protocolo interno do cluster não precisa seguir o protocolo externo.

## 4. Request lifecycle

```text
Client
  |
  v
API Gateway
  |
  v
Authentication / Authorization
  |
  v
Request Validation
  |
  v
Normalization
  |
  v
Edge Optimization
  |
  v
Model Resolution
  |
  v
Execution Planning
  |
  v
Scheduling
  |
  v
Inference
  |
  v
Streaming / Response
```

## 5. Request object

Todo request recebe um identificador global:

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

O `request_id` é usado em logs, métricas, traces, estado, grafo e auditoria.

## 6. Identificadores de execução

Uma request pode possuir mais de uma execução lógica devido a retry ou recovery.

```text
request_id
  |
  +--> execution_id #1
  +--> execution_id #2
```

A resposta final representa a request, enquanto a observabilidade mantém o histórico de cada execução.

## 7. Streaming

Streaming deve ser tratado como requisito de primeira classe.

```text
Client
  |
  +<-- token 1
  +<-- token 2
  +<-- token 3
  +<-- token N
```

O consumidor não precisa saber em quais nodes os tokens foram produzidos.

A API deve transportar metadados de uso de forma compatível com o cliente sem misturar métricas internas confidenciais na resposta padrão.

## 8. Cancellation

O cliente deve poder cancelar uma execução:

```text
POST/DELETE request cancellation
```

Fluxo interno:

```text
Client cancel
  -> API Gateway
  -> Request Manager
  -> Execution Manager
  -> cancel pending Work Units
  -> stop generation
  -> release reservations/state
  -> emit event
```

O cancelamento deve ser idempotente.

## 9. Timeout e deadline

A request pode indicar:

```text
request timeout
execution deadline
max output tokens
```

Esses valores alimentam o Scheduler, mas não devem ser usados para violar garantias de integridade de estado.

## 10. Prioridades

A API deve aceitar uma classe de prioridade controlada por política:

```text
realtime
interactive
normal
batch
background
```

O cliente não pode definir arbitrariamente prioridade crítica sem autorização.

## 11. Token accounting

Para cada request, o cluster deve registrar:

```text
input_tokens_received
input_tokens_after_optimization
cached_tokens
output_tokens_generated
tokens_avoided
tokens_total_processed
```

Exemplo:

```text
Input received           18,420
After optimization       5,210
Tokens avoided          13,210
Output generated         1,842
```

## 12. Métricas de latência

A API deve permitir correlacionar:

```text
request latency
TTFT
inter-token latency
generation throughput
edge latency
planning latency
queue latency
```

## 13. Error model

Os erros externos devem ser estáveis e simples, mesmo quando a causa interna for complexa.

Categorias:

```text
INVALID_REQUEST
AUTHENTICATION_FAILED
AUTHORIZATION_FAILED
MODEL_NOT_FOUND
MODEL_UNAVAILABLE
CAPACITY_UNAVAILABLE
TIMEOUT
CANCELLED
INTERNAL_ERROR
SERVICE_UNAVAILABLE
```

Detalhes internos permanecem em trace/event/log, e não devem vazar para o consumidor indiscriminadamente.

## 14. Model discovery

```http
GET /v1/models
```

A resposta deve expor somente modelos/variantes autorizados:

```json
{
  "id": "llama-x-3.2-q4",
  "object": "model",
  "owned_by": "ai-cluster",
  "context_length": 131072
}
```

A existência física dos shards e nodes não faz parte do contrato público.

## 15. Routing policy

O cliente pode solicitar:

```text
model
quality target
latency target
```

Mas o cliente não escolhe:

```text
node
shard
worker
transport
placement
```

Essas decisões pertencem ao cluster.

## 16. Observability headers

A API deve retornar, quando apropriado:

```text
request_id
trace_id
model_id
model_version
usage
```

O consumidor pode então correlacionar sua própria telemetria com o cluster.

## 17. Segurança

A API externa deve ter:

```text
TLS
authentication
authorization
rate limiting
request size limits
model ACL
quota enforcement
```

O acesso do cliente termina na API. Não deve haver exposição direta dos endpoints do Worker.

## 18. Multi-tenant isolation

Mesmo que o primeiro deployment seja single-tenant, o modelo deve permitir isolamento por:

```text
tenant
application
API key / identity
quota
model ACL
priority policy
```

Isso evita uma migração arquitetural futura caso múltiplos consumidores sejam adicionados.

## 19. API de administração

A administração não usa a Inference API.

```text
Inference API
  -> workload

Management API / CLI
  -> infrastructure
```

Exemplos de operações administrativas:

```text
nodes
health
models
plans
workers
metrics
traces
events
provisioning
recovery
```

## 20. Limites de responsabilidade

### API

Responsável por:

- receber requisições;
- autenticar e autorizar;
- validar;
- normalizar;
- iniciar/cancelar execução;
- transmitir resposta;
- expor uso e identificadores.

### Cluster interno

Responsável por:

- otimização de contexto;
- graph retrieval;
- model selection;
- planning;
- scheduling;
- state;
- transport;
- execução distribuída.

## 21. Health da API

A saúde da API deve ser separada da saúde do cluster.

Exemplo:

```text
API = GREEN
Cluster = YELLOW
Capacity = 72%
```

O endpoint de health não deve depender de executar inferência real em cada chamada.

## 22. Backpressure

Se não houver capacidade suficiente, a API deve usar política explícita:

```text
queue
reject
retry-after
fallback model
```

O cliente não deve abrir centenas de requisições em paralelo apenas para contornar capacidade insuficiente.

## 23. Fallback

O fallback interno pode existir:

```text
primary variant
  |
  +--> alternate variant
  +--> alternate placement
  +--> alternate model, se política permitir
```

Mas fallback de modelo deve ser explícito, porque pode alterar qualidade e comportamento.

## 24. Contrato com agentes

Hermes, Codex e outros agentes devem enxergar o cluster como um endpoint de inferência, não como uma coleção de workers.

```text
Agent
  |
  v
Inference API
  |
  v
AI Cluster
```

A API deve permanecer estável enquanto a topologia interna evolui.
