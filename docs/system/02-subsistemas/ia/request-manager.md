---
title: "Request Manager"
aliases: ["Gerenciador de requisições", "docs/24", "24 — Request Manager"]
tags: [sistema/subsistema/ia]
type: sistema/subsistema
status: decidido
implementacao: nao-iniciado
fases: ["[[fase-11-request-manager]]"]
---

# Request Manager

O Request Manager controla o ciclo de vida **externo** de uma requisição, da entrada na [[inference-api]] até a criação de uma Execution. Decide **se e em que condições** uma request pode executar; o [[scheduler]] decide **como** os recursos concretos são usados. Voltar para [[ia]].

## Responsabilidades

- Validar e normalizar requests; aplicar autorização trazida pelo gateway.
- Aplicar quotas e limites por identidade, tenant ou application.
- Estimar orçamento de tokens e recursos.
- Decidir admissão, fila ou rejeição.
- Criar `request_id` e `execution_id`; controlar cancelamento.
- Manter o estado de alto nível da request.
- Integrar [[edge-optimizer]], [[model-manager]], [[execution-planner]] e [[scheduler]].
- Publicar métricas e eventos.

## Fluxo

```text
Client -> Inference API -> Request Manager
  -> Admission Control -> Token/Resource Budget
  -> Edge Optimizer -> Model Resolution
  -> Execution Planner -> Scheduler -> Execution
```

## Estados da request

```text
RECEIVED  VALIDATING  ADMITTED  QUEUED  OPTIMIZING  PLANNING
RUNNING   STREAMING   CANCELLING  CANCELLED  COMPLETED  FAILED  REJECTED  EXPIRED
```

Os estados são idempotentes e persistidos no control plane (ver [[state-manager]]).

## Requisição versus execução

Request é a intenção externa; Execution é uma tentativa concreta. Cada execução tem seu plano, epochs, Work Units e trace. A request mantém o estado final observado pelo cliente. Em falha recuperável: [[state-manager]] recupera, nova Execution, novo plano, retomada; a request continua a mesma.

## Admissão

A request não vai ao Planner automaticamente. O Admission Controller avalia disponibilidade do modelo, capacidade do cluster, prioridade, quota, tamanho de contexto, saída estimada, deadline e carga atual.

Saídas: `ADMIT`, `QUEUE`, `REJECT`, `FALLBACK`. Fallback de modelo só se a política do consumidor permitir.

Rejeição antecipada, sem tocar o runtime: modelo não autorizado, requisição malformada, contexto grande demais, quota excedida, modalidade não suportada.

## Fila

Lógica, por classe de prioridade: `REALTIME`, `INTERACTIVE`, `NORMAL`, `BATCH`, `BACKGROUND`. Dentro da classe, aging contra starvation. Não é round-robin simples, porque jobs e recursos são heterogêneos.

## Quotas e orçamentos

- Quotas por identidade: requests simultâneas, tokens de entrada e saída por request, tokens por minuto, requests na fila, tempo de execução.
- Orçamento de tokens antes da execução: entrada recebida, entrada após otimização, saída esperada e máxima, total estimado. A estimativa não é garantia; alimenta planejamento e observabilidade.
- Orçamento de recursos: tempo de compute, memória, KV cache, tráfego de rede, staging de storage, latência esperada. Serve para rejeitar ou enfileirar o que claramente não cabe.
- Estimativa de custo (compute units, energia, I/O, custo externo) não é cobrança sem um módulo de billing.

## Edge, cancelamento e deadlines

- Ordem: validação barata → otimização de borda → recálculo do orçamento → admissão final. Reduzir contexto pode tornar viável uma request antes inviável.
- Cancelamento atravessa a cadeia (cliente → API → Request Manager → Execution → Scheduler → Work Units). Unidades não iniciadas são canceladas; em execução recebem sinal e o Worker escolhe o ponto seguro para parar.
- Deadline: `remaining_budget = deadline - elapsed_time`, passado ao Planner e ao Scheduler. Sem tempo plausível: enfileirar, replanejar, cair para fallback (se permitido) ou rejeitar.

## Backpressure e concorrência

```text
NORMAL         -> admit
HIGH LOAD      -> queue
CRITICAL       -> reject background/batch
```

O objetivo é proteger o cluster, não maximizar a fila. A concorrência considera requests, execuções, Work Units, capacidade do modelo, memória, KV cache e rede: limitar só o número de requests não basta, porque uma request longa consome muito mais que dezenas de pequenas.

## Prioridade e equidade

- Work Units herdam a classe de prioridade da request, sujeitas à política interna. Operação background nunca ganha prioridade de interactive sem política explícita.
- Equidade entre classes e tenants considera prioridade, tempo de espera, tamanho do recurso, fairness por tenant e deadline.

## Streaming e tokens

Para streaming: `RUNNING → primeiro token → STREAMING → COMPLETED`. O Request Manager acompanha TTFT e a emissão de chunks, sem participar de cada token. Ao final consolida entrada recebida, entrada otimizada, tokens em cache, saída e tokens evitados, no formato público.

## Idempotência

Requests com `Idempotency-Key` evitam duplicação quando o cliente repete após timeout de rede: a chave busca a request existente. A semântica precisa ser definida antes da API pública.

## Observabilidade

Eventos por transição: `REQUEST_RECEIVED`, `REQUEST_ADMITTED`, `REQUEST_QUEUED`, `REQUEST_OPTIMIZED`, `REQUEST_PLANNED`, `REQUEST_STARTED`, `REQUEST_CANCELLED`, `REQUEST_COMPLETED`, `REQUEST_FAILED`, `REQUEST_REJECTED`.

Métricas mínimas: latência de admissão, latência de fila, requests em execução, profundidade da fila, rejeições, cancelamentos e taxa de conclusão.

> [!info] O que já existe (MVP)
> Não há Request Manager. Há peças isoladas, todas no gateway ou no controller:
> - Gateway: admissão binária por semáforo. Embeddings com até 4 pedidos (`MAX_IN_FLIGHT`), chat com uma geração por vez (`llm_slot`); acima disso, 429 com `Retry-After`. Não há fila.
> - Gateway: prazo fixo de 30 s (`DEADLINE`) para embeddings e chat; estourar o prazo gera 504 `TIMEOUT` e, no chat, sinaliza cancelamento da geração no próximo token.
> - Controller: `EnqueueGraph` recusa com `ResourceExhausted` acima de 256 unidades na fila (`MAX_QUEUED_UNITS`); o gateway responde 429 com `Retry-After: 5`.
> - Controller: `ShardMatmul` limitado a 8 chamadas simultâneas (`MAX_SHARD_CALLS`), 64 operações por chamada e prazo de 8 s (`SHARD_TIMEOUT`).
> - Identificadores: `x-request-id` gerado no gateway. Uma request tem uma única execução; a recuperação de faixa re-chama o nó vivo dentro da mesma geração ([[f08-canal-residente-de-fatias]]).
> - Sem estados persistidos, sem prioridade, sem quota, sem orçamento de tokens, sem `Idempotency-Key`, sem eventos `REQUEST_*`.
>
> Slices: [[f08-gateway-e-api]], [[f08-canal-residente-de-fatias]].

> [!warning] Desvio do alvo e migração
> A admissão e a execução estão acopladas no gateway, e o controller só conhece Work Units, não requests. O controle de entrada é contagem fixa, sem orçamento de tokens nem fila por classe. Migração: [[fase-11-request-manager]], entre a API e o Scheduler, com estados persistidos pelo [[state-manager]].

> [!todo] Falta
> - Admission Controller com orçamento de tokens e recursos.
> - Fila lógica por classe com aging e equidade entre tenants.
> - Quotas por identidade e ACL de modelo.
> - Estados persistidos e eventos `REQUEST_*`.
> - Cancelamento por `request_id` que propaga até as Work Units ([[inference-api]]).
> - Deadline calculado (`remaining_budget`) em vez de prazo fixo.
> - `Idempotency-Key`.
