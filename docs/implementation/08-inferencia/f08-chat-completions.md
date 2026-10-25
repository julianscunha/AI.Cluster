---
title: "F08 — Chat completions e streaming"
aliases: ["Fase J3", "J3", "chat-completions", "streaming", "ChatML"]
tags: [implementacao/fase/08, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-08-inferencia]]"
codigos-antigos: ["J3", "J4"]
sistema: ["[[inference-api]]", "[[inference-engine]]"]
evidencia: ["code/tests/phase-j-chat.sh", "code/crates/engine/tests/smollm2_chat_parity.rs", "code/crates/engine/reference/smollm2_chat_reference.py"]
---
# F08 — Chat completions e streaming

- [ ] Superado

> [!note] Contexto
> A [[f08-gateway-e-api]] servia só o modelo sintético de embeddings. Esta fatia põe modelos de
> linguagem reais atrás de `POST /v1/chat/completions`, rodando **dentro do gateway**, com a mesma
> chave de API. Os modelos e o forward estão em [[f08-engine-local]]; a execução fatiada nos
> workers está em [[f08-canal-residente-de-fatias]].

## Objetivo

Expor conversa no formato OpenAI com GPT-2 small e SmolLM2-135M (base e instruct), com streaming
SSE, e com prazos e limites que protegem o gateway.

## O que foi feito

- O gateway carrega cada modelo na subida, por variável de diretório: `AI_CLUSTER_GPT2_DIR`
  (GPT-2 small, id `gpt2`), `AI_CLUSTER_SMOLLM2_DIR` (SmolLM2-135M, id `smollm2-135m`) e
  `AI_CLUSTER_SMOLLM2_INSTRUCT_DIR` (SmolLM2-135M-Instruct, id `smollm2-135m-instruct`). Falha
  fechada se um diretório estiver incompleto.
- Os modelos aparecem em `/v1/models` e são aceitos em `POST /v1/chat/completions`, com a mesma
  chave de API.
- Formato OpenAI: `model`, `messages` (`system`/`user`/`assistant`), `max_tokens` (padrão 16, teto
  64). Resposta com `choices[0].message`, `finish_reason` (`stop` no fim de texto, `length`) e
  `usage`.
- **Streaming.** `stream: true` devolve `text/event-stream` no formato OpenAI: um chunk com o papel,
  um `chat.completion.chunk` por trecho de texto, um com o `finish_reason` e `data: [DONE]`. Um
  caractere UTF-8 partido entre tokens só sai inteiro.
- Os cabeçalhos saem antes da geração. Uma falha depois disso chega como evento `{"error": ...}`
  seguido de `[DONE]`, não como status.
- Cliente que desconecta: a geração é cancelada no próximo token. Um stream com cliente parado
  também é tratado (`STREAM_STALL`, 10 s sem leitura).
- Não há `usage` no stream.
- **Modelos base** (GPT-2 e SmolLM2-135M) não têm template de chat: os conteúdos das mensagens são
  unidos por quebra de linha e o modelo **continua o texto**. A resposta é uma continuação.
- **Modelo instruct** (`smollm2-135m-instruct`) recebe a conversa em ChatML
  (`<|im_start|>papel\nconteúdo<|im_end|>\n`), com a mensagem de sistema padrão do modelo quando não
  há uma, e termina a resposta em `<|im_end|>` (`finish_reason: "stop"`).
- Os tokens especiais entram por id, e o texto das mensagens passa pelo codificador comum. Um
  `<|im_end|>` escrito pelo usuário é só texto: não fecha o próprio turno nem abre um de `system`.
- A paridade dos ids do prompt e da continuação com o `transformers` está em
  `code/crates/engine/tests/smollm2_chat_parity.rs`, com a referência em
  `smollm2_chat_reference.py`.

## Decisões

- **Geração gulosa e determinística.** Mesmo prompt, mesma saída. `temperature`, `n` e `stop` são
  ignorados (sempre gulosa, uma escolha).
- **Uma geração por vez, somando todos os modelos.** Simples e protege a RAM do gateway; ocupado
  responde 429 com `Retry-After`.
- **Ajustar, não rejeitar.** `max_tokens` fora de 1–64 é ajustado.
- **Modelo roda no gateway.** O custo é RAM (cerca de 1,6 GB) e o gateway vira o ponto único; a
  alternativa de fatiar os produtos nos workers é opcional, por `*_SHARDS`
  ([[f08-canal-residente-de-fatias]]).

## Evidência

`bash code/tests/phase-j-chat.sh [gpt2|smollm2|instruct]`. O script exige os pesos em
`code/models/<modelo>` (não versionados). Paridade de tokenizador e logits:
`code/crates/engine/tests/gpt2_parity.rs`, `smollm2_parity.rs` e `smollm2_chat_parity.rs`.

## Limites e próximos passos

- **Uma geração por vez** para todos os modelos juntos: não há fila nem prioridade; o 429 é o
  controle de entrada. A fila e a admissão por orçamento são da
  [[fase-11-request-manager]].
- **Prazo de 30 s** (504). Estourar sinaliza o cancelamento, que vale no próximo token; o slot só
  libera quando a geração termina.
- **Prompt de 1 a 512 tokens, até 8 KiB e até 32 mensagens.** Fora disso, `INVALID_REQUEST`
  (400); papel fora de `system`/`user`/`assistant` também.
- **Sem amostragem.** Temperatura, top-p e `n` não existem; a saída é sempre a mesma para o mesmo
  prompt.
- **Sem `usage` no stream** e `completion_tokens` não conta o token de fim de texto.
- **Modelo na RAM do gateway.** Sem [[fase-12-model-runtime]], o gateway concentra o modelo e os
  pesos ficam sem versão ([[f08-canal-residente-de-fatias]], "Limites").
- **Sem contagem de tokens por cliente** nem cota: pendente de [[fase-11-request-manager]].

> [!info] O que já existe
> `/v1/chat/completions` com GPT-2, SmolLM2 base e instruct, streaming SSE, template ChatML e
> cancelamento ao desconectar, em `code/crates/gateway/src/main.rs` e `code/crates/engine`.

## Relacionados

- Fase: [[fase-08-inferencia]] · Sistema: [[inference-api]], [[inference-engine]]
- Irmãos: [[f08-gateway-e-api]], [[f08-engine-local]], [[f08-canal-residente-de-fatias]]
