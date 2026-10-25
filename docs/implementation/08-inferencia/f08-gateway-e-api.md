# Inference API (Fase J1)

A porta HTTP do Application Plane (docs/23, ADR-007): aplicações falam com o gateway e nunca com
o controller nem com os workers.

## O que ela **não** é

Não há modelo de linguagem atrás dela. O único "modelo" é `matmul-diamond-v0`, marcado
`"kind": "synthetic-matmul"`: um *forward pass* sintético, o grafo diamante de multiplicação de
matrizes (16×16) que já existia, executado de verdade nos workers. O texto de entrada vira uma
semente (FNV-1a de 64 bits), o grafo roda, e a primeira linha da matriz da unidade final volta
como "embedding". O vetor **não tem semântica**: é determinístico (mesmo texto, mesmo vetor) e só
prova o caminho completo aplicação → gateway → controller → workers → resposta. Toda resposta
traz `"synthetic": true` e uma nota dizendo isso. `/v1/chat/completions` não existe de propósito:
exigiria inventar texto.

## O que existe

- Crate `ai-cluster-gateway`, binário `ai-cluster-gateway`. Fala com o controller como
  **operador** (`AI_CLUSTER_OPERATOR_TOKEN`) e herda o TLS do cliente (`AI_CLUSTER_TLS_CA`).
- `GET /v1/models`, `POST /v1/embeddings` (`{"model": "matmul-diamond-v0", "input": "texto"}`),
  `GET /health` (sem chave, não toca o cluster).
- Segurança (docs/23 §17): `Authorization: Bearer <AI_CLUSTER_API_KEY>`, comparada sem saída
  antecipada. A chave é obrigatória e **diferente** dos tokens de join e de operador: ela
  encara aplicações, os outros encaram o cluster. Corpo máximo de 64 KiB, `input` de 1 a 8192
  bytes, no máximo 4 pedidos em andamento (acima: 429 com `Retry-After`).
- A perna HTTP não tem TLS: o gateway escuta em `127.0.0.1:8080` e só aceita outro endereço com
  `AI_CLUSTER_INSECURE=1`.
- Erros estáveis (docs/23 §13): 401 `AUTHENTICATION_FAILED`, 404 `MODEL_NOT_FOUND`, 400/413
  `INVALID_REQUEST`, 429 `CAPACITY_UNAVAILABLE`, 504 `TIMEOUT` (30 s), 503 `MODEL_UNAVAILABLE`
  (unidade falhou) e `SERVICE_UNAVAILABLE` (controller inalcançável). A causa interna fica no log.
- Cabeçalhos `x-request-id` e `x-model-id`; o `request_id` também vem no corpo e leva a
  `ai-cluster request trace <id>`.
- No controller: `EnqueueGraph` ganhou `keep_result` e há o RPC `GetOutput` (só operador). O
  controller guarda a saída da unidade final só quando pedido, e só as últimas 64.

## Evidência

`bash code/tests/phase-j-inference.sh`: o gateway não sobe sem chave nem com a chave igual ao
token de join; sem worker o pedido responde 504; 401 sem chave; o modelo vem como sintético; o
vetor tem 16 números, é igual para o mesmo texto e diferente para outro; cabeçalhos de
correlação; 404, 400 e 413.

## Tetos e o que vem depois

Em ordem: `/v1/chat/completions` (quando houver modelo real e a primeira família escolhida),
entrada arbitrária no grafo, streaming, cancelamento, Request Manager (admissão e fila),
prioridades, cotas e contagem de tokens, ACL de modelo e multi-tenant.

- **Sem cancelamento:** um pedido que estoura o prazo deixa o grafo na fila do controller. O
  controller recusa `EnqueueGraph` com `ResourceExhausted` quando há mais de 256 unidades na
  fila (o gateway responde 429 com `Retry-After: 5`), mas os grafos já na fila rodam quando um
  worker aparece. A unidade que falha também solta o resultado que esperava guardar.
- **Uma chave só:** não há tenant, cota nem ACL. Qualquer portador da chave usa o modelo.
- **Resultado por `request_id`:** o `GetOutput` é do operador; o gateway só devolve ao cliente o
  resultado do pedido que ele mesmo criou.
- **Sem TLS na perna HTTP**, a chave trafega em claro fora de loopback.

## J3: `/v1/chat/completions` sobre modelos de linguagem

Com `AI_CLUSTER_GPT2_DIR` (GPT-2 small), `AI_CLUSTER_SMOLLM2_DIR` (SmolLM2-135M, id `smollm2-135m`)
e/ou `AI_CLUSTER_SMOLLM2_INSTRUCT_DIR` (SmolLM2-135M-Instruct, id `smollm2-135m-instruct`) apontando para os arquivos do modelo (veja `inference-engine.md`), o gateway
carrega cada um na subida (falha fechada se um diretório estiver incompleto) e passa a
listar o modelo em `/v1/models` e a aceitar `POST /v1/chat/completions` (mesma chave de API).

- Formato OpenAI: `model`, `messages` (`system`/`user`/`assistant`), `max_tokens` (padrão 16, teto
  64); resposta com `choices[0].message`, `finish_reason` (`stop` no fim de texto, `length`) e
  `usage`.
- **Streaming.** `stream: true` devolve `text/event-stream` no formato OpenAI: um chunk com o
  papel, um `chat.completion.chunk` por trecho de texto, um com o `finish_reason` e `data: [DONE]`.
  Um caractere UTF-8 partido entre tokens só sai inteiro. Os cabeçalhos saem antes da geração, então
  uma falha depois disso chega como evento `{"error": ...}` seguido de `[DONE]`, não como status.
  Se o cliente desconecta, a geração é cancelada no próximo token. Não há `usage` no stream.
- **Modelos base** (GPT-2, SmolLM2-135M) não têm template de chat: os conteúdos das mensagens são
  unidos por quebra de linha e o modelo **continua o texto**. A resposta é uma continuação.
- **Modelo instruct** (`smollm2-135m-instruct`) recebe a conversa em ChatML
  (`<|im_start|>papel\nconteúdo<|im_end|>\n`), com a mensagem de sistema padrão do modelo quando não há
  uma, e termina a resposta em `<|im_end|>` (`finish_reason: "stop"`). Os tokens especiais entram
  por id e o texto das mensagens passa pelo codificador comum: um `<|im_end|>` escrito pelo usuário é
  só texto e não consegue fechar o próprio turno nem abrir um de `system`. A paridade dos ids do
  prompt e da continuação com o `transformers` está em `tests/smollm2_chat_parity.rs`.
- Geração gulosa e determinística. Uma geração por vez, somando todos os modelos (429 com `Retry-After` se ocupado); prompt de
  até 512 tokens e 8 KiB; prazo de 30 s (504). Estourar o prazo sinaliza o cancelamento, que vale
  no próximo token; o slot só libera quando a geração termina. `max_tokens` fora de 1–64 é
  ajustado, não rejeitado; `temperature`, `n` e `stop` são ignorados (sempre gulosa, uma escolha).
  `completion_tokens` não conta o token de fim de texto.
- O modelo roda **dentro do gateway** (cerca de 1,6 GB de RAM); os produtos de matriz não passam
  pelo cluster (a menos que `*_SHARDS` ponha as projeções nos workers: `inference-engine.md`, J5). O
  E2E é `tests/phase-j-chat.sh [gpt2|smollm2|instruct]` e exige os pesos em `code/models/<modelo>`.
