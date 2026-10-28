---
title: "API HTTP do gateway"
aliases: ["API de inferência", "Gateway", "/v1/chat/completions"]
tags: [guia, api]
type: guia
---

# API HTTP do gateway

O gateway (`ai-cluster-gateway`) é a porta HTTP de inferência. Ele escuta em `AI_CLUSTER_GATEWAY_ADDR` (padrão `127.0.0.1:8080`). Volte ao [[guia]] para o índice; as variáveis estão em [[configuracao]].

## Autenticação

Toda rota, exceto `/health`, exige o cabeçalho:

```
Authorization: Bearer <chave>
```

A chave identifica um tenant. Ela define quais modelos ele pode usar, a cota de tokens e a classe máxima de prioridade. Chave ausente ou inválida recebe `401`.

## Rotas

| Método e rota | Auth | Para quê |
|---|---|---|
| `GET /health` | não | Saúde da API. Não consulta o cluster. Responde `{"api":"green"}`. |
| `GET /v1/models` | sim | Modelos que a chave pode usar. |
| `POST /v1/chat/completions` | sim | Geração de texto (modelos de linguagem). |
| `POST /v1/embeddings` | sim | Vetor do modelo sintético `matmul-diamond-v0`. |
| `GET /metrics` | sim | Métricas de geração e de cota, em formato Prometheus. |

### `GET /v1/models`

Lista os modelos visíveis para a chave: `{"object":"list","data":[...]}`. Cada item traz `id`, `kind`, `description` e, para modelos de linguagem, `version` (quando há manifest verificado).

- `matmul-diamond-v0` (`kind: synthetic-matmul`) aparece sempre que a chave tem acesso a ele. Não é um modelo de linguagem: o vetor não tem semântica.
- `gpt2`, `smollm2-135m` e `smollm2-135m-instruct` (`kind: language-model`) aparecem só se estiverem configurados e se a chave tiver acesso.

### `POST /v1/chat/completions`

Corpo:

```json
{
  "model": "smollm2-135m-instruct",
  "messages": [{"role": "user", "content": "Olá"}],
  "max_tokens": 16,
  "stream": false
}
```

- `model`: obrigatório. Um dos ids de `/v1/models`.
- `messages`: de 1 a 32 mensagens. `role` é `system`, `user` ou `assistant`. O texto total fica entre 1 e 8192 bytes.
- `max_tokens`: padrão 16, limitado a 1..64.
- `stream`: padrão `false`. Com `stream_options: {"include_usage": true}`, um último evento (sem `choices`) traz o `usage`.
- `temperature`: de 0 a 2, padrão 0. Zero é guloso: o mesmo prompt dá sempre a mesma resposta. Acima de 0 a resposta é sorteada.
- `top_p`: de 0 (exclusive) a 1, padrão 1. Mantém só os tokens mais prováveis que somam essa massa. Só importa com `temperature` > 0.
- `seed`: inteiro sem sinal. A mesma semente, com os mesmos parâmetros, repete a resposta sorteada. Sem ela, cada pedido sorteia a sua.
- `n`: de 1 a 4 respostas alternativas (`choices`), geradas em sequência no mesmo slot; só sem `stream`.
- O prompt tem de caber em 1 a 512 tokens.

Campos não listados são ignorados. Valor fora da faixa de `temperature`, `top_p` ou `n` dá `400`. Só a resposta gulosa (`temperature` 0 e `n` 1) vai ao cache.

Modelos de linguagem base (`gpt2`, `smollm2-135m`) não têm template de chat: as mensagens são unidas por quebra de linha e o modelo continua o texto. Modelos `instruct` usam o template ChatML.

Resposta sem streaming (`stream: false`):

```json
{
  "id": "chatcmpl-1",
  "object": "chat.completion",
  "created": 1767225600,
  "model": "smollm2-135m-instruct",
  "choices": [{"index": 0, "message": {"role": "assistant", "content": "..."}, "finish_reason": "stop"}],
  "usage": {"prompt_tokens": 12, "completion_tokens": 9, "total_tokens": 21}
}
```

`finish_reason` é `stop` (o modelo terminou) ou `length` (atingiu `max_tokens`). `completion_tokens` conta o token de fim de texto, que também foi gerado (o texto devolvido não o traz).

Com `stream: true`, a resposta é `text/event-stream`. Cada evento é `data: {...}` com `object: "chat.completion.chunk"` e `choices[0].delta`:

1. Primeiro evento: `delta` com `role: "assistant"`.
2. Eventos seguintes: `delta` com `content` (texto novo).
3. Último evento: `delta` vazio e `finish_reason` (`stop` ou `length`). Não há `usage` no stream.
4. Linha final: `data: [DONE]`.

Se a geração falhar no meio, o stream manda `data: {"error":{"type":...,"message":...}}` e em seguida `data: [DONE]`, sem evento de `finish_reason`. Se o cliente desconectar, a geração é cancelada.

### `POST /v1/embeddings`

Corpo: `{"model": "matmul-diamond-v0", "input": "texto"}`. `input` tem de 1 a 8192 bytes.

Resposta: um vetor de 16 números, mais `request_id`, `synthetic: true` e uma nota dizendo que o vetor não tem semântica. Usa o token de operador do gateway; sem ele a rota responde `503 EMBEDDINGS_DISABLED`.

### `GET /metrics`

Texto Prometheus com as métricas de geração (`ai_cluster_inference_*`: requisições, falhas, tokens, TTFT e ITL em p50 e p95, tokens por segundo, bytes do cache KV), do cache de respostas (`ai_cluster_cache_*`) e os contadores do próprio tenant (`ai_cluster_tenant_tokens_total`, `ai_cluster_tenant_rejected_total`). Os números zeram quando o gateway reinicia.

## Cabeçalhos

| Cabeçalho | Onde | Sentido |
|---|---|---|
| `Authorization: Bearer` | toda requisição | Chave de API. |
| `x-priority` | chat e embeddings | Classe do pedido: `background`, `batch`, `normal`, `interactive` ou `realtime` (sem diferenciar maiúsculas). Padrão `normal`. Não passa do teto do tenant. Só muda a ordem de espera e de despacho. |
| `x-request-id` | toda resposta de inferência | Id do pedido. O mesmo aparece no corpo e serve a `ai-cluster request trace`. |
| `x-model-id` | respostas de inferência | Id do modelo usado. |
| `x-model-version` | respostas de inferência | Versão do manifest do modelo, quando houver. |
| `Retry-After` | respostas `429` | Segundos a esperar antes de tentar de novo. |

## Limites e cotas

- Corpo da requisição: 64 KiB no máximo.
- Cota de tokens por tenant (`tokens=N` em `AI_CLUSTER_TENANTS`): conta o prompt mais o gerado. Embeddings cobram cerca de um token por 4 bytes de `input`. A cota é conferida antes de cada pedido, então o último pedido de uma janela pode passar do limite pelo próprio tamanho.
- Chat: uma geração por vez para todos os modelos. Até `AI_CLUSTER_QUEUE_MAX` pedidos esperam a vez, por no máximo `AI_CLUSTER_QUEUE_WAIT_SECS`. Além disso, recebem `429`.
- Embeddings: 4 em andamento ao mesmo tempo; o quinto recebe `429` com `Retry-After: 1`.
- Prazo de cada geração: 30 s. Passando disso, `504`.
- Cache de respostas: um pedido idêntico ao anterior do mesmo tenant (mesmo modelo, versão, prompt e `max_tokens`) é servido da memória, sem gerar de novo e sem esperar a vez. A cota é cobrada do mesmo jeito. Vale para o stream também.

## Erros

Todo erro vem como JSON `{"error": {"type": "...", "message": "..."}}`. A causa interna fica no log do gateway.

| HTTP | `type` | Quando |
|---|---|---|
| 400 | `INVALID_REQUEST` | Corpo inválido, mensagens ou `input` fora dos limites, prompt vazio ou acima de 512 tokens. |
| 401 | `AUTHENTICATION_FAILED` | Chave ausente ou inválida. |
| 403 | `MODEL_FORBIDDEN` | A chave não tem acesso ao modelo. |
| 404 | `MODEL_NOT_FOUND` | Modelo desconhecido. |
| 413 | `INVALID_REQUEST` | Corpo acima de 64 KiB. |
| 415 | `INVALID_REQUEST` | `Content-Type` diferente de JSON. |
| 429 | `QUOTA_EXCEEDED` | Cota de tokens da janela esgotada. `Retry-After` diz quando a janela renova. |
| 429 | `CAPACITY_UNAVAILABLE` | Sem vaga: fila de chat cheia ou espera esgotada, muitos embeddings em andamento, ou fila de unidades do controller cheia. `Retry-After` de 1 a 5 s. |
| 503 | `MODEL_UNAVAILABLE` | Uma unidade ou a geração falhou no cluster. |
| 503 | `EMBEDDINGS_DISABLED` | Embeddings pedidos sem token de operador configurado. |
| 503 | `SERVICE_UNAVAILABLE` | Controller inalcançável. |
| 504 | `TIMEOUT` | A geração ou o grafo não terminou em 30 s. |

O gateway não sobe sem chave de API ou tenant, e não sobe sem pelo menos um dos tokens de operador ou de inferência. Veja [[operacao]] para a diferença entre cada token.
