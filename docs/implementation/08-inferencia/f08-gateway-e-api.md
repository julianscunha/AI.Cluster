---
title: "F08 — Gateway e API de inferência"
aliases: ["inference-api", "Fase J1", "J1"]
tags: [implementacao/fase/08, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-08-inferencia]]"
codigos-antigos: ["J1"]
sistema: ["[[inference-api]]", "[[adr-007-fronteira-inference-api]]"]
evidencia: ["code/tests/phase-j-inference.sh"]
---
# F08 — Gateway e API de inferência

- [ ] Superado

> [!note] Contexto
> A porta HTTP do Application Plane ([[inference-api]], ADR-007): aplicações falam com o gateway e
> nunca com o controller nem com os workers. Esta é a primeira fatia da F08; o modelo de linguagem
> entra depois, em [[f08-chat-completions]].

## Objetivo

Expor uma API HTTP de inferência que prova o caminho completo aplicação → gateway → controller →
workers → resposta, com autenticação própria e erros estáveis.

## O que foi feito

- Crate `ai-cluster-gateway`, binário `ai-cluster-gateway`. Fala com o controller como **operador**
  (`AI_CLUSTER_OPERATOR_TOKEN`) e herda o TLS do cliente (`AI_CLUSTER_TLS_CA`).
- `GET /v1/models`, `POST /v1/embeddings` (`{"model": "matmul-diamond-v0", "input": "texto"}`) e
  `GET /health` (sem chave; não toca o cluster).
- Segurança (ver [[inference-api#Segurança e isolamento]]): `Authorization: Bearer <AI_CLUSTER_API_KEY>`, comparada sem
  saída antecipada. A chave é obrigatória e **diferente** dos tokens de join e de operador: ela
  encara aplicações, os outros encaram o cluster.
- Limites: corpo máximo de 64 KiB, `input` de 1 a 8192 bytes, no máximo 4 pedidos em andamento
  (acima: 429 com `Retry-After`).
- Erros estáveis (ver [[inference-api#Modelo de erro]]): 401 `AUTHENTICATION_FAILED`, 404 `MODEL_NOT_FOUND`,
  400/413 `INVALID_REQUEST`, 429 `CAPACITY_UNAVAILABLE`, 504 `TIMEOUT` (30 s), 503
  `MODEL_UNAVAILABLE` (unidade falhou) e `SERVICE_UNAVAILABLE` (controller inalcançável). A causa
  interna fica no log.
- Cabeçalhos `x-request-id` e `x-model-id`; o `request_id` também vem no corpo e leva a
  `ai-cluster request trace <id>`.
- No controller: `EnqueueGraph` ganhou `keep_result`, e há o RPC `GetOutput` (só operador). O
  controller guarda a saída da unidade final só quando pedido, e só as últimas 64.

## Decisões

- **Modelo sintético primeiro.** `matmul-diamond-v0` (`"kind": "synthetic-matmul"`) é um forward
  pass sintético: o grafo diamante de multiplicação de matrizes 16×16 executado de verdade nos
  workers. O texto vira semente (FNV-1a de 64 bits), e a primeira linha da matriz da unidade final
  volta como "embedding". O vetor **não tem semântica**: é determinístico (mesmo texto, mesmo
  vetor) e prova só o caminho. Toda resposta traz `"synthetic": true` e uma nota dizendo isso.
- **Chave de API separada.** Aplicação não usa o token de operador nem o de join.
- **Gateway não é o cluster.** Ele não escolhe nó: manda o grafo ao controller e espera.

## Evidência

`bash code/tests/phase-j-inference.sh`:

- o gateway não sobe sem chave nem com a chave igual ao token de join;
- sem worker, o pedido responde 504;
- 401 sem chave;
- o modelo vem como sintético;
- o vetor tem 16 números, é igual para o mesmo texto e diferente para outro;
- cabeçalhos de correlação;
- 404, 400 e 413.

## Limites e próximos passos

- **Sem cancelamento de grafo.** Um pedido de embedding que estoura o prazo deixa o grafo na fila
  do controller. O controller recusa `EnqueueGraph` com `ResourceExhausted` quando há mais de 256
  unidades na fila (o gateway responde 429 com `Retry-After: 5`), mas os grafos já na fila rodam
  quando um worker aparece. A unidade que falha também solta o resultado que esperava guardar.
  Cancelamento de pedido chega com o Request Manager ([[fase-11-request-manager]]).
- **Uma chave só.** Não há tenant, cota nem ACL. Qualquer portador da chave usa o modelo. Cotas e
  multi-tenant são da [[fase-11-request-manager]].
- **Resultado por `request_id`.** O `GetOutput` é do operador; o gateway só devolve ao cliente o
  resultado do pedido que ele mesmo criou.
- **Sem TLS na perna HTTP.** A chave trafega em claro fora de loopback. O gateway só aceita outro
  endereço que `127.0.0.1:8080` com `AI_CLUSTER_INSECURE=1`. TLS para essa perna está pendente em
  [[fase-19-hardening-de-producao]].
- **Entrada arbitrária no grafo** não existe: o grafo é o sintético. Ver
  [[f08-chat-completions]] para o modelo de linguagem real.

> [!info] O que já existe
> Gateway HTTP com chave de API, `/v1/models`, `/v1/embeddings` e `/health`, com erros estáveis e
> cabeçalhos de correlação. Os modelos de linguagem estão em [[f08-chat-completions]].

## Relacionados

- Fase: [[fase-08-inferencia]] · Sistema: [[inference-api]]
- Irmãos: [[f08-chat-completions]], [[f08-engine-local]], [[f08-canal-residente-de-fatias]]
