---
title: "Canal residente de fatias"
aliases: ["Canal de fatias", "canal residente", "shard channel"]
tags: [sistema/protocolo, subsistema/inferencia]
type: sistema/protocolo
---

# Canal residente de fatias

Canal pelo qual o gateway faz uma projeção de modelo ser calculada nos workers, sem enviar pesos. Os pesos de cada projeção ficam no disco e na memória dos nós; por ele trafegam só as ativações e as saídas. Decisão em [[adr-009-canal-residente-de-fatias]]; papel do gateway em [[inference-api]]; motor em [[inference-engine]]. Índice: [[protocolo]].

## Por que existe

Um decode de token executa centenas de multiplicações dependentes. Com a fila de trabalho (puxar, depois devolver com `SubmitWorkResult`), cada operação dependente custaria dezenas de milissegundos, e um token levaria segundos. Um canal aberto pelo worker, que empurra quadros, reduz cada chamada a uma ida e volta de rede mais o cálculo.

Num único host o caminho remoto ainda é mais lento que o local. Medido entre o PC (gateway) e um Orange Pi (worker em cabo) com o SmolLM2-135M-Instruct em duas faixas, uma em cada máquina: a pré-carga de um pedido (~25 linhas por chamada) fez 92 chamadas em 29,8 s, ou 324 ms por chamada, dos quais 185 ms são cálculo no Pi e 139 ms transporte e serialização (as ativações e saídas somam ~0,5 MB por chamada em `double`). Nenhum token saiu dentro do prazo de 30 s do gateway. O motor próprio com fatias em placas ARM fracas serve à paridade numérica e a pesquisa, não a uso: o caminho de produção é o llama.cpp por RPC. O ganho aparece quando o modelo não cabe num nó ou quando o lote é grande.

## Topologia

```
gateway --ShardMatmul(x, ops)--> controller --ShardCompute--> worker (stream residente)
        <------ saídas ---------            <--ShardResult---
```

- O worker abre `ShardChannel` (stream bidirecional gRPC). Não há porta no worker.
- O gateway não conhece worker: manda operações ao controller, que as roteia pelo canal do nó indicado em cada `ShardOp`.
- Projeções com a mesma entrada vão numa chamada só: q, k e v; gate e up. Assim a SmolLM2 faz 4 idas e voltas por camada e o GPT-2 faz 4 por bloco.

## Quadros

Não há framing próprio: cada quadro é uma mensagem Protobuf no stream gRPC. Os tipos estão em [[mensagens]].

| Sentido | Mensagem | Conteúdo |
|---|---|---|
| gateway → controller | `ShardMatmulRequest` | `rows`, `k`, `x` (`rows × k`, linha a linha), `ops[]` |
| controller → worker | `ShardCompute` | `op_id` (do controller), `model_subdir`, `tensor`, `row_start`, `row_end`, `rows`, `k`, `x`, `by_columns` |
| worker → controller | `ShardResult` | `op_id`, `ok`, `error`, `rows`, `cols`, `data`, `compute_us`, `invalid` |
| controller → gateway | `ShardMatmulResponse` | `outputs[]` na mesma ordem de `ops` |

Semântica da faixa:

- Padrão: `tensor` é `[out, in]` (SmolLM2, Llama). A faixa `[row_start, row_end)` é de linhas, e a saída é `x × faixa^T`, com forma `rows × (row_end − row_start)`.
- `by_columns`: `tensor` é `[in, out]` (GPT-2, `Conv1D`). A faixa é de colunas e a fatia fica como `[in, faixa]`, sem transposição.
- O gateway junta as saídas por colunas, na ordem das faixas.
- A chave residente de uma fatia é `(model_subdir, tensor, row_start, row_end, by_columns)`. Não há versão a sincronizar: o arquivo é o mesmo em todos os nós que o hospedam.

## Autenticação e autorização

| Caminho | Exige |
|---|---|
| Abertura de `ShardChannel` | nó no metadado `x-ai-cluster-node-id`, registrado, aprovado, elegível e com a credencial exata do nó |
| `ShardMatmul` | token de inferência ou de operador |
| `ShardCapacity` | token de inferência ou de operador |
| `ReportGateway` | token de inferência ou de operador: o gateway se apresenta ao controller (id, URL, modelos e nós das fatias) a cada 15 s. `ListGateways`, que lê isso, é só do operador. |
| Qualquer outro RPC com o token de inferência | `PermissionDenied`: o token só passa para `ShardMatmul` e `ShardCapacity` |

- O token de operador **não** abre o canal: sem a credencial do nó, `ShardChannel` é recusado.
- Um RPC novo fica fora do alcance do token de inferência até alguém liberá-lo. Isso vale por construção, não por lista de exceções.

## Sessão do canal

- Uma sessão por nó. Abrir outra substitui a anterior, e as operações que esperavam na antiga falham.
- Ao fechar, a sessão só sai se ainda for a atual: um stream já substituído não derruba o sucessor.
- `revoke` e `leave` derrubam a sessão e as operações pendentes.
- Depois de registrar a sessão, o controller confere a elegibilidade de novo, para que um nó revogado no intervalo não fique com sessão.
- Keepalive HTTP/2 de 10 s nos dois lados detecta conexão meio aberta.
- O worker reconecta a cada 2 s. Um motivo de recusa é impresso uma vez, não a cada tentativa.

## Admissão por capacidade

Ao subir, o gateway confere se os nós do modelo comportam as fatias, em vez de deixar o erro aparecer na primeira chamada.

- O modelo calcula quanto cada nó guarda: as fatias ficam residentes em `f64`, então o custo é `linhas da faixa × elementos por linha × 8` somado nas projeções.
- O worker reporta no registro quanto aceita guardar (`shard_cache_bytes`, vindo de `AI_CLUSTER_SHARD_CACHE_MB`; zero sem `AI_CLUSTER_MODEL_DIR`).
- O gateway chama `ShardCapacity` com os nós do modelo. O controller responde, para cada um, se está disponível (registrado, aprovado e vivo), o cache de fatias e a RAM. Só devolve o que o próprio nó reportou.
- O pedido cabe se `bytes ≤ min(cache, RAM)` (a RAM só entra se foi reportada). Senão o gateway recusa a subida: `nó w1 precisa de 3.0 GB de fatias e aceita 2.0 GB (faltam 1.0 GB)`.
- Falha fechada: nó ausente, indisponível ou com cache zero (worker antigo) também recusa. Nós que ainda estão registrando ganham até 30 s.
- A carga do modelo já recusa uma faixa que passe do teto de 256 MiB por fatia, e o gateway só deixa 80% da RAM do nó para as fatias.
- O token de inferência passa a ler, dos nós que nomeia, se estão vivos e quanto aceitam guardar; não enumera nós, porque precisa conhecer o id.
- Worker antigo (sem o campo de cache) recusa o modelo fatiado: atualize os workers antes do gateway.
- Não cobre o failover (a faixa de um nó que cai vai para os vivos sem reserva de espaço) nem uma mudança de cache depois da subida.

## Relay no controller

| Limite | Valor | Ao estourar |
|---|---|---|
| operações por `ShardMatmul` | 64 | `InvalidArgument` |
| valores de `x` | 2 Mi | `InvalidArgument` |
| linhas por operação | 2^20 | `InvalidArgument` |
| valores de saída por operação | 8 Mi | `InvalidArgument` |
| valores de saída por chamada | 16 Mi | `InvalidArgument` |
| chamadas `ShardMatmul` simultâneas | 8 | `ResourceExhausted` |
| quadros na fila de um nó | 64 | `ResourceExhausted` ("fila do canal cheia") |
| operações em voo por nó | 64 | `ResourceExhausted` |
| prazo de uma chamada inteira | 8 s (`AI_CLUSTER_SHARD_TIMEOUT_MS`, 1 s a 5 min) | `DeadlineExceeded` |

Regras de relay:

- O controller atribui um `op_id` próprio. Uma resposta só vale se chegar do nó para o qual a operação foi enviada.
- Qualquer término da chamada (resposta, erro, prazo ou cliente que sai) esquece as operações ainda pendentes e manda ao worker um quadro `ShardCompute` com `cancel = true` e o `op_id`: o worker pula a operação que ainda espera vaga de cálculo. Um cálculo já em curso termina e a resposta é descartada. Se o gateway cancelar, a chamada em voo é abortada.
- `ShardMatmul` não entra em `GetMetrics` nem em `GetTrace`, que contam unidades de trabalho. A visibilidade é o evento `shard_channel_opened` em `ListEvents` e o log do gateway.
- O prazo de 30 s do gateway vale para a requisição inteira. Cada nova tentativa do gateway recebe seu próprio prazo de 8 s.

## Worker

| Item | Valor |
|---|---|
| Ativação (`x`) | 2 Mi valores |
| Cálculos simultâneos | 2; cada um usa todos os núcleos |
| Fila de respostas | 64 |
| Cache de fatias | `AI_CLUSTER_SHARD_CACHE_MB`, padrão 2048 MiB |
| Tamanho de uma fatia | até 256 MiB |
| Saída de um cálculo | até 64 MiB |

**Carga.** Na primeira vez que uma faixa é pedida, o worker lê só essas linhas (ou colunas) de `<raiz>/<model_subdir>/model.safetensors`, usando cabeçalho e `seek`, e guarda a faixa transposta, em `[in, faixas]`. Uma carga por vez: duas cargas simultâneas da mesma faixa não duplicam o trabalho.

**Caminho.** `model_subdir` aceita só componentes normais. A raiz e o arquivo são canonicalizados, e o resultado precisa continuar sob a raiz: um link simbólico ou uma junção não escapa. O nome do arquivo é fixo.

**Allowlist de tensores.** O worker aceita só 11 sufixos de projeção de camada: `attn.c_attn`, `attn.c_proj`, `mlp.c_fc`, `mlp.c_proj` (GPT-2), e `self_attn.q_proj`, `self_attn.k_proj`, `self_attn.v_proj`, `self_attn.o_proj`, `mlp.gate_proj`, `mlp.up_proj`, `mlp.down_proj` (Llama). Embeddings, normalizações e o resto do disco ficam fora do alcance.

**Cache LRU.** Cada entrada guarda o instante do último uso. Ao carregar com a cache cheia, despeja as menos usadas até caber. Uma fatia em uso continua viva enquanto alguém a usa e é recarregada no próximo uso. Fatia maior que a cache é recusada com erro de nó.

## Erros e quem é culpado

O controller marca a falha com `x-ai-cluster-failed-node` quando a culpa é de um nó. O gateway refaz a chamada em outros nós só para erros acusados com código `Unavailable`, `DeadlineExceeded` ou `FailedPrecondition`.

| Causa | Código | Acusa nó | Gateway |
|---|---|---|---|
| Nó sem canal aberto | `FailedPrecondition` | sim | refaz e põe o nó de castigo por 30 s |
| Nó indisponível antes do envio | `FailedPrecondition` | sim | refaz |
| Canal caiu ou fechou no meio | `Unavailable` | sim | refaz |
| Worker não respondeu em 8 s | `DeadlineExceeded` | sim | refaz |
| Erro do nó: modelo ausente no disco, leitura falhou, fatia maior que a cache, saída acima de 64 MiB | `FailedPrecondition` | sim | refaz |
| Worker recusou o pedido (`invalid`): tensor fora da lista, entrada incoerente | `InvalidArgument` | não | 503, sem castigo |
| Forma inesperada da saída | `Internal` | não | 503 |
| Fila do canal ou operações em voo cheias | `ResourceExhausted` | sim, sem retentativa | 503 |
| Limite de chamadas simultâneas | `ResourceExhausted` | não | 503 |
| Limite de tamanho ou de operações | `InvalidArgument` | não | 503 |
| Token ausente ou inválido | `Unauthenticated` / `PermissionDenied` | não | 503 |

- Cada nova tentativa tira mais um nó de castigo, então o laço termina depois da última tentativa possível.
- Se todos os nós estiverem de castigo, o gateway tenta todos de novo, em vez de recusar pelo tempo do castigo. Um nó que está mesmo fora falha rápido e encerra as tentativas.
- Qualquer worker calcula qualquer faixa, porque lê do próprio disco. Mover uma faixa não copia nada.

> [!warning] Recusa do pedido não é culpa do nó
> `invalid` existe para isso: outro nó recusaria o mesmo pedido. Por isso o gateway devolve 503 na hora, sem castigo e sem nova tentativa.

## Token de inferência e InferenceGate

- `AI_CLUSTER_INFERENCE_TOKEN` é um terceiro segredo, opcional. Serve só para `ShardMatmul`: deixa o gateway rodar modelos nos workers sem o token de operador.
- O gateway só com esse token não aprova nó nem enfileira trabalho. `/v1/embeddings`, que enfileira o grafo sintético, responde 503 nesse modo.
- Com os dois tokens, o gateway usa o mais estreito.
- O token escolhe nó, subdiretório, tensor e faixa. O controller limita a concorrência (8 chamadas) e o worker limita as projeções. Não há cota por token.

## Configuração

| Variável | Onde | Efeito |
|---|---|---|
| `AI_CLUSTER_MODEL_DIR` | worker | liga o canal e define a raiz dos modelos |
| `AI_CLUSTER_SHARD_CACHE_MB` | worker | teto da cache de fatias (padrão 2048) |
| `AI_CLUSTER_INFERENCE_TOKEN` | controller | token de inferência |
| `AI_CLUSTER_SMOLLM2_SHARDS=worker-1,worker-2` | gateway | liga o modo remoto da SmolLM2 |
| `AI_CLUSTER_SMOLLM2_SUBDIR` | gateway | diretório da SmolLM2 nos workers (padrão `smollm2`) |
| `AI_CLUSTER_GPT2_SHARDS` | gateway | liga o modo remoto do GPT-2 |
| `AI_CLUSTER_GPT2_SUBDIR` | gateway | diretório do GPT-2 nos workers (padrão `gpt2`) |

O log `remoto:` do gateway mostra as chamadas, o tempo gasto nelas e o cálculo medido pelos workers.

## Garantias

- Um forward fatiado em três nós, lendo os arquivos, produz logits idênticos aos do forward local, para SmolLM2 e GPT-2.
- Um nó que some no meio da geração não derruba a requisição: o gateway refaz a operação em outro nó, ou responde 503 sem ficar pendurado.
- Um gateway só com o token de inferência não lista nós nem enfileira trabalho.

## Limites conhecidos

- **Sem réplica de fatias.** Um nó que herda uma faixa a lê do disco na primeira vez, com alguns segundos de aquecimento.
- **Sem rebalanceamento proativo.** Quando um nó entra, a faixa só volta a ele quando o castigo de 30 s do nó anterior acaba.
- **Quarentena e saída preparada não derrubam a sessão.** Só `revoke` e `leave` derrubam. Operações para nó indisponível são recusadas pelo `ShardMatmul`.
- **Só divisão por linhas de saída.** A entrada é replicada, sem soma entre nós nem pipeline entre camadas.
- **Token de inferência com poder amplo.** Vazado, pode forçar o despejo da cache e gerar muitas leituras por coluna. O worker limita as projeções e o controller limita a concorrência, mas não há cota por token.
- **Faixas movidas podem passar de 64 operações em voo por nó.** Com muitos nós e poucos vivos, a chamada recebe 503.
- **Rede real não caracterizada.** A latência e o custo de rede numa LAN física não foram medidos.

> Planejado: paralelismo de tensor e de pipeline entre camadas; cota por token de inferência; réplica e rebalanceamento proativo de fatias.
