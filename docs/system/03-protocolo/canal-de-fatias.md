---
title: "Canal residente de fatias"
aliases: ["Canal de fatias", "inference-engine §J5", "canal residente"]
tags: [sistema/protocolo, subsistema/inferencia]
type: sistema/protocolo
status: experimental
implementacao: parcial
fases: ["[[fase-08-inferencia]]"]
---

# Canal residente de fatias

Especificação do canal pelo qual o gateway faz uma projeção de modelo ser calculada nos workers, sem enviar pesos. Os pesos de cada projeção ficam residentes no disco e na memória dos nós; por ele trafegam só as ativações e as saídas. Decisão em [[adr-009-canal-residente-de-fatias]]; papel do gateway em [[inference-api]]; motor em [[inference-engine]]. Índice: [[protocolo]].

## Por que existe

Um decode de token executa centenas de multiplicações dependentes. A fila de trabalho (puxar, depois devolver por `SubmitWorkResult`) custa dezenas de ms por operação dependente, o que daria segundos por token. Um canal aberto pelo worker, que empurra quadros, reduz cada chamada a uma ida e volta de rede mais o cálculo. Números medidos em [[f08-engine-local]]: SmolLM2-135M, 2 workers, 120 chamadas por token, ~2,5 ms por chamada, ~300 ms por token contra ~105 ms local. Ou seja, ainda 3 vezes mais lento num único host; o ganho só aparece quando o modelo não cabe num nó ou o lote é grande.

## Topologia

```
gateway --ShardMatmul(x, ops)--> controller --ShardCompute--> worker (stream residente)
        <------ saídas ---------            <--ShardResult---
```

- O worker abre `ShardChannel` (stream bidirecional gRPC). Não há porta no worker.
- O gateway não conhece worker: manda operações ao controller, que as roteia pelo canal do nó indicado em cada `ShardOp`.
- Projeções com a mesma entrada vão numa chamada só: q, k e v; gate e up. São 4 idas e voltas por camada na SmolLM2 e 4 por bloco no GPT-2 (nada compartilha entrada ali).

## Framing

Não há framing próprio: cada quadro é uma mensagem Protobuf no stream gRPC. Os tipos estão em [[mensagens]].

| Sentido | Mensagem | Conteúdo |
|---|---|---|
| gateway → controller | `ShardMatmulRequest` | `rows`, `k`, `x` (`rows × k`, linha a linha), `ops[]` |
| controller → worker | `ShardCompute` | `op_id` (do controller), `model_subdir`, `tensor`, `row_start`, `row_end`, `rows`, `k`, `x`, `by_columns` |
| worker → controller | `ShardResult` | `op_id`, `ok`, `error`, `rows`, `cols`, `data`, `compute_us`, `invalid` |
| controller → gateway | `ShardMatmulResponse` | `outputs[]` na mesma ordem de `ops` |

Semântica da faixa:

- Padrão: `tensor` é `[out, in]` (SmolLM2, Llama); a faixa `[row_start, row_end)` é de linhas. A saída é `x × faixa^T`, com `rows × (row_end − row_start)`.
- `by_columns`: `tensor` é `[in, out]` (GPT-2 `Conv1D`); a faixa é de colunas e a fatia já fica como `[in, faixa]`, sem transposição.
- O gateway junta as saídas por colunas, na ordem das faixas.
- Chave residente: `(model_subdir, tensor, row_start, row_end, by_columns)`. Não há versão a sincronizar: o arquivo é o mesmo em todos os nós que o hospedam.

## Autenticação e autorização

| Caminho | Exige |
|---|---|
| Abertura de `ShardChannel` | nó no metadado `x-ai-cluster-node-id`, registrado, aprovado, em estado elegível e com a credencial exata do nó |
| `ShardMatmul` | token de inferência ou de operador (`require_inference`) |
| Qualquer outro RPC com o token de inferência | `PermissionDenied`: o `InferenceGate` deixa esse token passar só para `ShardMatmul` |

- O token de operador **não** abre o canal: sem a credencial do nó, `ShardChannel` é recusado.
- Os três tokens de servidor (join, operador, inferência) são distintos; o controller não sobe se coincidirem.
- Um RPC novo fica fora do alcance do token de inferência até alguém liberá-lo, por construção.

## Sessão do canal

- Uma sessão por nó. Abrir outra substitui a anterior e falha as operações que esperavam nela.
- Ao fechar, a sessão só sai se ainda for a atual: um stream já substituído não derruba o sucessor.
- `revoke` e `leave` derrubam a sessão e as operações pendentes.
- Após o registro da sessão o controller confere a elegibilidade de novo, para que um nó revogado no intervalo não fique com sessão.
- Keepalive HTTP/2 de 10 s nos dois lados detecta conexão meio-aberta.
- O worker reconecta a cada 2 s. Motivo de recusa é impresso uma vez por motivo distinto, não a cada tentativa.

## Relay no controller

Limites (constantes em `code/crates/controller/src/service.rs` e `shards.rs`):

| Limite | Valor | Ao estourar |
|---|---|---|
| `MAX_SHARD_OPS` operações por `ShardMatmul` | 64 | `InvalidArgument` |
| `MAX_SHARD_X` valores de `x` | 2 Mi (`2 × 1024 × 1024`) | `InvalidArgument` |
| `MAX_SHARD_ROWS` linhas por operação | 2^20 | `InvalidArgument` |
| `MAX_SHARD_OUT` valores de saída por operação | 8 Mi | `InvalidArgument` |
| Valores de saída por chamada | 16 Mi (`2 × MAX_SHARD_OUT`) | `InvalidArgument` |
| `MAX_SHARD_CALLS` chamadas `ShardMatmul` simultâneas | 8 | `ResourceExhausted` |
| `CHANNEL_DEPTH` quadros na fila de um nó | 64 | `ResourceExhausted` ("fila do canal cheia") |
| `MAX_PENDING_PER_NODE` operações em voo por nó | 64 | `ResourceExhausted` |
| `SHARD_TIMEOUT` prazo de uma chamada inteira | 8 s | `DeadlineExceeded` |

Regras de relay:

- O controller atribui um `op_id` próprio. Uma resposta só vale se chegar do nó para o qual a operação foi enviada.
- Qualquer término da chamada (resposta, erro, prazo ou cliente que sai) esquece as operações ainda pendentes (`ShardGuard`). O cancelamento do gateway aborta a chamada em voo.
- O controller não conta `ShardMatmul` em `GetMetrics` nem em `GetTrace`, que contam unidades de trabalho. A visibilidade é o evento `shard_channel_opened` em `ListEvents` e o log do gateway.

> [!warning] Prazo: a spec anterior dizia 60 s
> A versão anterior deste trecho dava prazo de 60 s. O código usa `SHARD_TIMEOUT` = 8 s por chamada. O prazo de 30 s do gateway vale para a requisição inteira, não para uma chamada de fatia. Cada nova tentativa do gateway recebe seu próprio prazo de 8 s.

## Worker

| Item | Valor | Onde |
|---|---|---|
| Ativação: `MAX_X` | 2 Mi valores | `code/crates/worker/src/shards.rs` |
| Cálculos simultâneos | 2 (`CONCURRENT_COMPUTES`), cada um já usa todos os núcleos | idem |
| Fila de respostas | 64 (`OUTBOX`) | idem |
| Cache de fatias | `AI_CLUSTER_SHARD_CACHE_MB`, padrão 2048 MiB | `code/crates/worker/src/main.rs` |
| Tamanho de uma fatia | até 256 MiB (`MAX_SLICE_BYTES`) | `code/crates/engine/src/shard.rs` |
| Saída de um cálculo | até 64 MiB (`MAX_OUTPUT_BYTES`) | idem |

**Carga.** Na primeira vez que uma faixa é pedida, o worker lê só essas linhas (ou colunas) de `root/<subdir>/model.safetensors` (cabeçalho e `seek`) e a guarda transposta, em `[in, faixas]`. Uma carga por vez: duas cargas simultâneas da mesma faixa não duplicam o trabalho.

**Caminho.** `subdir` aceita só componentes normais; a raiz e o arquivo são canonicalizados e o resultado precisa continuar sob a raiz (um link simbólico ou junção não escapa). O nome do arquivo é fixo.

**Allowlist de tensores.** `is_projection` aceita só 11 sufixos de projeção de camada: `attn.c_attn`, `attn.c_proj`, `mlp.c_fc`, `mlp.c_proj` (GPT-2); `self_attn.q/k/v/o_proj`, `mlp.gate/up/down_proj` (Llama). Embeddings, normas e o resto do disco ficam fora do alcance.

**Cache LRU.** Cada entrada guarda o instante do último uso. Ao carregar com a cache cheia, despeja as menos usadas até caber. Uma fatia em uso continua viva pelo `Arc` de quem a usa e é recarregada no próximo uso. Fatia maior que a cache é recusada com erro de nó.

## Erros e quem acusa o nó

O controller marca a falha com `x-ai-cluster-failed-node` quando a culpa é de um nó. O gateway (`GrpcExecutor`) refaz a chamada nos outros nós só para erros acusados e com código `Unavailable`, `DeadlineExceeded` ou `FailedPrecondition`.

| Causa | Código no controller | Acusa nó | Gateway |
|---|---|---|---|
| Nó sem canal aberto | `FailedPrecondition` | sim | refaz, põe o nó de lado por 30 s |
| Nó indisponível antes do envio | `FailedPrecondition` | sim | refaz |
| Canal caiu ou fechou no meio | `Unavailable` | sim | refaz |
| Worker não respondeu em 8 s | `DeadlineExceeded` | sim | refaz |
| Erro do nó: modelo ausente no disco, leitura falhou, fatia maior que a cache, saída acima de 64 MiB | `FailedPrecondition` | sim | refaz |
| Worker recusou o pedido (`invalid`): tensor fora da lista, entrada incoerente | `InvalidArgument` | não | falha com 503, sem pôr nó de lado |
| Forma inesperada da saída | `Internal` | não | 503 |
| Fila do canal ou operações em voo cheias | `ResourceExhausted` | sim, mas não retentável | 503 |
| Limite de chamadas simultâneas (`MAX_SHARD_CALLS`) | `ResourceExhausted` | não | 503 |
| Limite de tamanho ou de operações | `InvalidArgument` | não | 503 |
| Token ausente ou inválido | `Unauthenticated` / `PermissionDenied` | não | 503 |

- Cada nova tentativa tira mais um nó (`BENCH` = 30 s), então o laço termina depois da última tentativa possível.
- Se todos os nós estiverem de castigo, o gateway tenta todos de novo em vez de recusar pelo tempo do castigo. Um nó que está mesmo fora falha rápido e encerra as tentativas.
- Qualquer worker calcula qualquer faixa, já que lê do próprio disco. Mover uma faixa não copia nada.

> [!warning] Recusa do pedido não é culpa do nó
> `invalid` existe para isso: outro nó recusaria o mesmo pedido, então não faz sentido pôr o nó de lado nem refazer. O gateway devolve 503 na hora, sem castigo.

## Papel de inferência e InferenceGate

- `AI_CLUSTER_INFERENCE_TOKEN` é um terceiro segredo, opcional. Serve só para `ShardMatmul`, ou seja, deixa o gateway rodar modelos nos workers sem o token de operador.
- O gateway com só esse token não aprova nó nem enfileira trabalho. `/v1/embeddings`, que enfileira o grafo sintético, fica desligado (503) nesse modo.
- Com os dois tokens, o modelo usa o mais estreito.
- O token escolhe nó, subdiretório, tensor e faixa. O controller limita a concorrência (8 chamadas) e o worker limita as projeções; não há cota por token.

## Configuração

| Variável | Onde | Efeito |
|---|---|---|
| `AI_CLUSTER_MODEL_DIR` | worker | liga o canal e define a raiz dos modelos |
| `AI_CLUSTER_SHARD_CACHE_MB` | worker | teto da cache de fatias (padrão 2048) |
| `AI_CLUSTER_INFERENCE_TOKEN` | controller | token de inferência |
| `AI_CLUSTER_SMOLLM2_SHARDS=worker-1,worker-2` | gateway | liga o modo remoto da SmolLM2; `AI_CLUSTER_SMOLLM2_SUBDIR` (padrão `smollm2`) |
| `AI_CLUSTER_GPT2_SHARDS` / `AI_CLUSTER_GPT2_SUBDIR` | gateway | liga o modo remoto do GPT-2 |

O log `remoto:` do gateway mostra chamadas, tempo nas chamadas e o cálculo medido pelos workers.

## Verificação

- Forward fatiado em 3 nós, lendo os arquivos, dá logits idênticos ao local (`sharded_projections_match_the_local_forward`, SmolLM2 e GPT-2).
- `code/tests/phase-j-shards.sh` sobe controller, 2 workers e gateway: mesma continuação do local; 503 sem pendurar quando um worker some; mesma continuação com o worker morto no meio da geração; gateway só com o token de inferência não lista nós.
- Testes unitários de sessão, limites e recusa de capacidade estão em `shards.rs` e `service.rs` do controller.

> [!info] O que já existe
> - Canal, relay, gateway e allowlist: [[f08-canal-residente-de-fatias]], [[f08-gateway-e-api]].
> - Motor e paridade: [[f08-engine-local]].
> - Papéis de credencial: [[adr-010-papeis-de-credencial]], [[f07-operador-e-credencial-por-no]].
> - Fronteira de inferência: [[adr-007-fronteira-inference-api]].

> [!todo] Limites conhecidos e o que falta
> - **Sem réplica de fatias**: o nó que herda uma faixa a lê do disco na primeira vez; segundos de aquecimento.
> - **Sem rebalanceamento proativo**: quando um nó entra, a faixa só volta ao dono quando o castigo de 30 s acaba.
> - **Quarentena e drain não derrubam a sessão**: só `revoke` e `leave` derrubam. Operações para nó indisponível são recusadas pelo `ShardMatmul`.
> - **Só divisão por linhas de saída**: entrada replicada, sem all-reduce nem pipeline entre camadas (tensor e pipeline ficam em [[fase-12-model-runtime]]).
> - **Token de inferência com poder amplo**: vazado, pode forçar o despejo da cache (LRU) e gerar leituras por coluna (um `seek` por linha). Hoje o worker limita as projeções e o controller limita a concorrência; falta cota por token.
> - **Faixas movidas podem passar de 64 operações em voo por nó**: com muitos nós e poucos vivos, a chamada dá 503.
> - **Custo de rede não medido em LAN real**: o número acima é de contêineres numa máquina.
