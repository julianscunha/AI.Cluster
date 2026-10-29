---
title: "Inference API"
aliases: ["API de inferência", "Inference API", "Gateway de inferência"]
tags: [sistema/subsistema/ia]
type: sistema/subsistema
---

# Inference API

A Inference API é a fronteira entre as aplicações (agentes, orquestradores, CLIs) e o cluster. Nenhum cliente fala com o controller, com workers, com fatias de modelo ou com o estado interno. Ver [[adr-007-fronteira-inference-api]]. Voltar para [[ia]].

Hoje a API é o binário `ai-cluster-gateway`, um servidor HTTP.

## Papel

- Expõe o cluster como um endpoint de inferência estável. A topologia interna pode mudar sem mudar o contrato.
- Autentica, valida, admite e transmite a resposta. Não escolhe nó, faixa, transporte nem placement.
- Administração não usa esta API. Nós, métricas, eventos e aprovação ficam na CLI ([[cli-tui]]).

## Endpoints

| Método e rota | Autenticação | Descrição |
|---|---|---|
| `GET /health` | Não | Liveness. Não toca o cluster. |
| `GET /v1/models` | Chave | Lista só os modelos que o consumidor pode chamar, com a versão dos pesos. |
| `POST /v1/chat/completions` | Chave | Conversa com GPT-2 small, SmolLM2-135M e SmolLM2-135M-Instruct. Aceita `stream: true` (SSE). |
| `POST /v1/embeddings` | Chave | Embedding do modelo sintético `matmul-diamond-v0`. Ver abaixo. |
| `GET /metrics` | Chave | Métricas de inferência em texto Prometheus. Ver [[observabilidade]]. |

> Planejado: `POST /v1/responses`, `POST /v1/rerank`, `POST /v1/transcriptions` e `POST /v1/generate`.

## Configuração

| Variável | Padrão | Função |
|---|---|---|
| `AI_CLUSTER_GATEWAY_ADDR` | `127.0.0.1:8080` | Endereço de escuta. Fora de loopback exige TLS (`AI_CLUSTER_GATEWAY_TLS_CERT`/`_KEY`, PEM) ou `AI_CLUSTER_INSECURE=1`, porque a perna HTTP não tem TLS. |
| `AI_CLUSTER_CONTROLLER` | `http://127.0.0.1:50051` | Endpoint gRPC do controller. |
| `AI_CLUSTER_API_KEY` | — | Chave do tenant `default`: todos os modelos, sem cota. |
| `AI_CLUSTER_TENANTS` | — | Tenants adicionais, no formato `nome=chave[,models=a\|b][,tokens=N][,class=c];...`. |
| `AI_CLUSTER_OPERATOR_TOKEN` | — | Necessário para o modelo sintético de embeddings. |
| `AI_CLUSTER_INFERENCE_TOKEN` | — | Necessário para os modelos de linguagem, se o gateway usar o papel de inferência. |
| `AI_CLUSTER_TLS_CA` | — | CA que o gateway usa ao falar com o controller. |
| `AI_CLUSTER_GPT2_DIR`, `AI_CLUSTER_SMOLLM2_DIR`, `AI_CLUSTER_SMOLLM2_INSTRUCT_DIR` | — | Diretórios dos modelos. Carregados na subida; um diretório incompleto impede o início. |
| `AI_CLUSTER_GPT2_SHARDS`, `AI_CLUSTER_SMOLLM2_SHARDS`, `AI_CLUSTER_SMOLLM2_INSTRUCT_SHARDS` | — | Nós (`worker-1,worker-2`) que recebem as faixas de projeção. Sem isso, o modelo roda inteiro no gateway. |
| `AI_CLUSTER_GPT2_SUBDIR`, `AI_CLUSTER_SMOLLM2_SUBDIR`, `AI_CLUSTER_SMOLLM2_INSTRUCT_SUBDIR` | `gpt2`, `smollm2`, `smollm2-instruct` | Subdiretório dos pesos em cada worker. |
| `AI_CLUSTER_REQUIRE_MANIFEST` | desligado | Recusa modelo sem `manifest.json`. Ver [[model-manager]]. |
| `AI_CLUSTER_CACHE_ENTRIES` | `256` | Tamanho do cache de respostas. `0` desliga. Ver [[edge-optimizer]]. |
| `AI_CLUSTER_QUOTA_WINDOW_SECS` | `3600` | Janela da cota de tokens. |
| `AI_CLUSTER_QUEUE_MAX` | `8` | Pedidos de chat que podem esperar na fila. |
| `AI_CLUSTER_QUEUE_WAIT_SECS` | `10` | Tempo máximo de espera na fila. |
| `AI_CLUSTER_INSECURE` | desligado | Libera endereço não loopback e conexão sem TLS com o controller. |

A chave de API é obrigatória, e o gateway não sobe sem ela (nem sem um tenant configurado). Ver [[seguranca]] para a separação entre chaves de aplicação e tokens do cluster.

## Autenticação e tenants

- Cada requisição traz `Authorization: Bearer <chave>`. A comparação é feita sem saída antecipada.
- Cada tenant tem nome, chave, lista de modelos (ACL), limite de tokens por janela e classe máxima de prioridade.
- Um tenant vê em `GET /v1/models` só os modelos da sua ACL. Chamar um modelo fora dela responde 403 `MODEL_FORBIDDEN`.
- Chaves de tenant precisam ser distintas entre si e distintas dos tokens de join, operador e inferência. O gateway recusa subir se não forem.

## Modelos

| `model` | Tipo | Observação |
|---|---|---|
| `gpt2` | Linguagem, base | GPT-2 small (124M). Sem template de chat. |
| `smollm2-135m` | Linguagem, base | Família Llama. Sem template de chat. |
| `smollm2-135m-instruct` | Linguagem, instruct | Recebe conversa em ChatML. |
| `matmul-diamond-v0` | Sintético | Usado por `/v1/embeddings`. Não tem semântica. |

Cada modelo carregado aparece com a versão do manifesto (`version`). Sem manifesto, o campo vem vazio.

### Modelo do llama.cpp (`model start`)

Além dos acima, o gateway serve **um** modelo GGUF pelo `llama-server` (ADR-022), com o id que `model start` define (o nome do arquivo sem `.gguf`). Ele aparece em `GET /v1/models` com `engine: llama.cpp`, `context`, `state` (`loading`, `ready`, `failed`) e `tokens_per_second` (benchmark). Diferenças em relação aos modelos do motor próprio:

- **Corpo repassado inteiro** ao `llama-server` (o gateway só confere autenticação, modelo, cota e fila): vale tudo o que o llama.cpp aceita no `/v1/chat/completions`, incluindo `tools`. Em stream, o gateway força `stream_options.include_usage` para cobrar a cota.
- **Chaves:** as do controller (`model start`) valem para este modelo, e as do ambiente (`AI_CLUSTER_API_KEY`, `AI_CLUSTER_TENANTS`) continuam valendo para os demais.
- **Vagas:** `AI_CLUSTER_LLAMA_SLOTS` gerações ao mesmo tempo (fila própria, com a mesma regra de classe e espera). Mais vagas aumentam a vazão total e alongam cada resposta.
- **Recarga e queda de worker:** um pedido que chega com o modelo carregando espera até `AI_CLUSTER_LLAMA_WAIT_SECS` (padrão 120, máximo 600) antes do 503. Se o `llama-server` cai sem ter respondido nada (worker que caiu), o gateway espera a recarga e refaz o pedido uma vez; medido no Orange Pi, um pedido cortado por `kill -9` no `rpc-server` terminou com 200 em 179 s. Um stream já iniciado não é refeito. Não há recarga sem queda: o `rpc-server` atende um cliente por vez, então o servidor novo não sobe enquanto o antigo segura os workers.
- **Estados:** carregando devolve 503 `MODEL_LOADING` com `Retry-After: 10`; falhou, 503 `MODEL_UNAVAILABLE`; parado, o modelo some da lista (404).
- **Cota:** cobrada pelo `usage` do servidor ao fim da resposta; se o cliente desiste antes, cobra uma estimativa (um token a cada quatro bytes enviados).
- **Sem limite de 64 tokens nem de 512 no prompt:** valem os do llama.cpp e o contexto escolhido.

## Chat completions

Campos aceitos: `model`, `messages` (papéis `system`, `user` e `assistant`), `max_tokens`, `stream`, `stream_options.include_usage`, `temperature` (0 a 2, padrão 0 = guloso), `top_p`, `seed` e `n` (1 a 4, sem `stream`). Amostrar é do `engine` (`sampling.rs`: softmax com temperatura, núcleo, semente splitmix64); só a resposta gulosa entra no cache.

- `max_tokens` tem padrão 16 e vai de 1 a 64. Valor fora da faixa é ajustado, não rejeitado.
- Resposta sem stream: `choices[0].message`, `finish_reason` (`stop` ou `length`) e `usage`.
- Os modelos base recebem as mensagens unidas por quebra de linha e continuam o texto.
- O modelo instruct recebe ChatML (`<|im_start|>papel ... <|im_end|>`) com a mensagem de sistema padrão quando não há uma. Termina a resposta em `<|im_end|>`. Um `<|im_end|>` digitado pelo usuário é só texto.
- Campos `temperature`, `top_p`, `n` e `stop` são ignorados. A geração é gulosa, com uma escolha.

### Streaming

Com `stream: true` a resposta é `text/event-stream` no formato OpenAI: um chunk com o papel, um `chat.completion.chunk` por trecho de texto, um chunk final com `finish_reason` e `data: [DONE]`. Um caractere UTF-8 partido entre tokens só sai completo.

- Os cabeçalhos saem antes da geração. Uma falha depois disso chega como evento `{"error": ...}` seguido de `[DONE]`.
- Cliente que desconecta cancela a geração no próximo token.
- Cliente que para de ler por 10 s também cancela.
- O stream não traz `usage`.

### Embeddings

`POST /v1/embeddings` com `{"model": "matmul-diamond-v0", "input": "texto"}`. O grafo diamante de multiplicação de matrizes 16×16 roda nos workers. O texto vira semente, e a resposta traz um vetor de 16 números, determinístico para o mesmo texto. O vetor não tem semântica: a resposta sempre traz `"synthetic": true`. Entrada de 1 a 8192 bytes.

## Limites de requisição

| Item | Limite |
|---|---|
| Corpo da requisição | 64 KiB |
| Entrada de embedding | 1 a 8192 bytes |
| Embeddings em andamento | 4 |
| Gerações de chat em andamento | 1, para todos os modelos |
| Prompt de chat | 1 a 512 tokens, até 8 KiB e até 32 mensagens |
| `max_tokens` | 1 a 64 (ajustado) |
| Prazo de embedding e de chat | 30 s |
| Cliente sem ler o stream | 10 s |

Acima dos tetos de admissão o gateway responde 429 com `Retry-After`.

## Cota e fila

- A cota conta tokens (prompt mais gerados) por tenant e por janela. Embeddings gastam um token a cada quatro bytes de entrada. Cache hit também é cobrado, então o cache não contorna a cota.
- Esgotada a cota, a resposta é 429 `QUOTA_EXCEEDED` com `Retry-After` igual ao tempo que falta na janela.
- A geração de chat é atendida por classe de prioridade: `realtime`, `interactive`, `normal`, `batch` e `background`. A classe vem do cabeçalho `x-priority`. Sem ele vale `normal`, e a classe nunca passa do teto do tenant. Quem espera sobe uma classe a cada 5 s, para não ser preterido para sempre.
- Fila cheia ou espera acima de `AI_CLUSTER_QUEUE_WAIT_SECS` responde 429 `CAPACITY_UNAVAILABLE`.
- Prioridade de embedding também é enviada ao controller, que a usa no despacho.

## Erros

Os códigos são estáveis. A causa interna vai para o log, não para a resposta.

| Status | Código | Quando |
|---|---|---|
| 400, 413 | `INVALID_REQUEST` | Corpo malformado, entrada fora dos limites, papel desconhecido, corpo acima de 64 KiB. |
| 401 | `AUTHENTICATION_FAILED` | Chave ausente ou inválida. |
| 403 | `MODEL_FORBIDDEN` | Chave sem acesso ao modelo pedido. |
| 404 | `MODEL_NOT_FOUND` | Modelo desconhecido. |
| 429 | `CAPACITY_UNAVAILABLE` | Sem vaga de geração, fila cheia ou espera estourada. Traz `Retry-After`. |
| 429 | `QUOTA_EXCEEDED` | Cota da janela esgotada. Traz `Retry-After`. |
| 503 | `MODEL_UNAVAILABLE` | Falha de unidade de trabalho ou de nó que impede a resposta. |
| 503 | `SERVICE_UNAVAILABLE` | Controller inalcançável. |
| 504 | `TIMEOUT` | Prazo de 30 s estourado. |

## Cabeçalhos

- `x-request-id`: identificador da requisição, também no corpo. Leva a `ai-cluster request trace <id>` ([[cli-tui]]).
- `x-model-id` e `x-model-version`: modelo e versão dos pesos que responderam.
- `x-priority`: classe pedida pelo cliente (entrada).
- `Retry-After`: em respostas 429.

## Cancelamento e prazo

- Não há endpoint de cancelamento para o cliente. Desconectar ou estourar o prazo cancela a geração de chat no próximo token.
- Uma requisição de embedding que não entrega resposta (prazo, falha ou cliente que saiu) tem as unidades ainda na fila do controller removidas. Unidade já em execução num worker termina.

> Planejado: `DELETE` por `request_id`, propagando o cancelamento até as unidades em execução.

## Saúde

`GET /health` responde sem chave e não executa inferência nem consulta o cluster. A saúde da API é separada da saúde do cluster.

## Limites conhecidos

- **Autenticação só por chave.** A perna HTTP não tem TLS. Fora de loopback, a chave viaja em claro.
- **Sem `usage` no stream** e sem contabilização separada de tokens em cache ou evitados.
- **Sem `temperature`, `top_p`, `n` nem `stop`.** A saída é sempre a mesma para o mesmo prompt.
- **Erros que a API ainda não emite:** `CANCELLED`, `AUTHORIZATION_FAILED` e `INTERNAL_ERROR`.
- **Cota e contadores vivem na memória do gateway.** Reiniciar zera a cota. Com mais de um gateway, cada um conta por conta própria.
- **O último pedido da janela pode passar da cota**, pelo tamanho dele: a conta é feita ao fim da geração.
- **Sem `Idempotency-Key`.** Um cliente que repete após timeout gera outra execução.

> Planejado: TLS na perna HTTP, certificados por cliente, `usage` no stream e `Idempotency-Key`.
