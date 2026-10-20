---
title: "AI Cluster — Request Manager"
type: spec
status: decided
tags: [spec, request-manager]
---

# AI Cluster — Request Manager

Status: DECIDED baseline / v0.1

## 1. Objetivo

O Request Manager controla o ciclo de vida externo de uma requisição desde sua entrada na Inference API até a criação de uma Execution. Ele atua como fronteira entre API e o motor interno de execução.

Responsabilidades principais:

- validar e normalizar requests;
- aplicar autenticação/autorização resultante do gateway;
- aplicar quotas e limites;
- estimar orçamento de tokens e recursos;
- decidir admissão, fila ou rejeição;
- criar request e execution IDs;
- controlar cancelamento;
- manter estado de alto nível da request;
- integrar Edge Optimizer, Model Manager, Planner e Scheduler;
- publicar métricas e eventos.

## 2. Fluxo

```text
Client
  -> Inference API
  -> Request Manager
  -> Admission Control
  -> Token/Resource Budget
  -> Edge Optimizer
  -> Model Resolution
  -> Execution Planner
  -> Scheduler
  -> Execution
```

## 3. Estados da request

```text
RECEIVED
VALIDATING
ADMITTED
QUEUED
OPTIMIZING
PLANNING
RUNNING
STREAMING
CANCELLING
CANCELLED
COMPLETED
FAILED
REJECTED
EXPIRED
```

Estados devem ser idempotentes e persistíveis no control plane.

## 4. Request versus Execution

Uma Request representa a intenção externa. Uma Execution representa uma tentativa concreta de executá-la.

```text
Request
  |
  +-- Execution 1
  +-- Execution 2 (retry/recovery)
```

A Request mantém o estado final observado pelo cliente. Cada Execution possui seu próprio plano, epochs, work units e trace.

## 5. Controle de admissão

A request não deve ser enviada ao Planner automaticamente.

O Admission Controller avalia:

```text
model availability
cluster capacity
priority
quota
context size
estimated output
deadline
current load
```

Saídas possíveis:

```text
ADMIT
QUEUE
REJECT
FALLBACK
```

Fallback de modelo só pode ocorrer se permitido pela política do consumidor.

## 6. Fila

A fila deve ser lógica e orientada por classe de prioridade.

```text
REALTIME
INTERACTIVE
NORMAL
BATCH
BACKGROUND
```

Dentro de uma classe, o sistema pode usar aging para impedir starvation.

A fila não deve ser round-robin simples, porque a capacidade dos jobs e dos recursos é heterogênea.

## 7. Quotas

O Request Manager deve aplicar limites por identidade/tenant/application.

Exemplos:

```text
max concurrent requests
max input tokens/request
max output tokens/request
max total tokens/minute
max queued requests
max execution time
```

## 8. Orçamento de tokens

Antes da execução, o sistema calcula:

```text
input tokens received
input tokens after edge optimization
expected output tokens
maximum output tokens
estimated total
```

A estimativa não é garantia. Ela alimenta planejamento e observabilidade.

## 9. Orçamento de recursos

Além de tokens, pode haver estimativa de:

```text
compute time
memory footprint
KV cache
network traffic
storage staging
expected latency
```

Isso permite rejeitar ou enfileirar requests que claramente não cabem na capacidade atual.

## 10. Integração com Edge Optimization

A otimização ocorre depois da validação inicial, mas antes da alocação pesada.

Fluxo:

```text
received
  -> cheap validation
  -> edge optimization
  -> recompute token/resource estimate
  -> admission/planning final
```

Uma otimização que reduza o contexto pode tornar uma request anteriormente inviável executável.

## 11. Rejeição antecipada

Requests obviamente inválidas devem ser rejeitadas sem tocar o runtime de inferência.

Exemplos:

```text
model not authorized
malformed request
context too large
quota exceeded
unsupported modality
```

## 12. Backpressure

Quando a capacidade cai, o Request Manager controla entrada.

Exemplo:

```text
NORMAL
  -> admit

HIGH LOAD
  -> queue

CRITICAL CAPACITY
  -> reject low-priority/background
```

O objetivo é proteger o cluster contra sobrecarga, não apenas maximizar a fila.

## 13. Cancelamento

Cancelamento deve atravessar toda a cadeia:

```text
Client
  -> API
  -> Request Manager
  -> Execution
  -> Scheduler
  -> Work Units
```

Work Units não iniciadas são canceladas. Work Units em execução recebem sinal de cancelamento; o Worker decide o ponto seguro para abortar.

## 14. Deadlines

A request pode trazer deadline. O Request Manager deve calcular:

```text
remaining_budget = deadline - elapsed_time
```

Esse valor é passado ao Planner/Scheduler.

Se não houver tempo suficiente para completar de forma plausível, a request pode ser:

```text
queued
replanned
fallbacked, if policy allows
rejected
```

## 15. Herança de prioridade

Work Units criadas a partir de uma request devem herdar sua classe de prioridade, sujeitas às políticas internas.

Uma operação background nunca deve ganhar prioridade de interactive apenas porque pertence a uma request de alto nível, salvo política explícita.

## 16. Controle de concorrência

A concorrência deve considerar:

```text
requests
executions
work units
model capacity
memory
KV cache
network
```

Não basta limitar apenas número de requests.

Uma request longa pode consumir muito mais recursos que dezenas de requests pequenas.

## 17. Streaming lifecycle

Para streaming:

```text
RUNNING
  -> first token
  -> STREAMING
  -> more tokens
  -> COMPLETED
```

O Request Manager acompanha TTFT e emissão de chunks, mas não participa de cada token individual além do necessário para controle.

## 18. Contabilização de tokens

Ao final, o Request Manager consolida:

```text
received input tokens
optimized input tokens
cached tokens
output tokens
tokens avoided
```

O valor retornado ao consumidor deve ser compatível com a API pública.

## 19. Estimativa de custo

Se houver um modelo de custo, o Request Manager pode estimar:

```text
compute units
energy estimate
storage I/O
network transfer
external model/provider cost
```

Esses valores não devem ser apresentados como cobrança real sem um módulo específico de billing.

## 20. Recuperação

Se a Execution falhar e for recuperável:

```text
Execution failed
  -> State Manager recovery
  -> new Execution
  -> new plan
  -> resume
```

A Request permanece a mesma.

## 21. Idempotência

Requests com chave de idempotência devem evitar duplicação acidental quando o cliente repetir uma chamada após timeout de rede.

```text
Idempotency-Key
  -> existing request lookup
```

A semântica deve ser definida antes do MVP público.

## 22. Equidade

O Request Manager deve evitar starvation entre classes e tenants.

O algoritmo exato poderá evoluir, mas deve considerar:

```text
priority
waiting time
resource size
tenant fairness
deadline
```

## 23. Observabilidade

Cada transição importante gera evento:

```text
REQUEST_RECEIVED
REQUEST_ADMITTED
REQUEST_QUEUED
REQUEST_OPTIMIZED
REQUEST_PLANNED
REQUEST_STARTED
REQUEST_CANCELLED
REQUEST_COMPLETED
REQUEST_FAILED
REQUEST_REJECTED
```

Métricas mínimas:

```text
admission latency
queue latency
running requests
queue depth
rejections
cancellations
completion rate
```

## 24. Separation from Scheduler

O Request Manager decide **se e em que condições uma request pode executar**.

O Scheduler decide **como recursos concretos serão usados para executar o plano**.

```text
Request Manager
  -> admission / queue / budget

Scheduler
  -> placement / ordering / resource assignment
```

Essa separação evita concentrar toda a lógica de controle em um único componente.
