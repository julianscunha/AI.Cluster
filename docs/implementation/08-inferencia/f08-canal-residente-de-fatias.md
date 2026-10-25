---
title: "F08 — Canal residente de fatias"
aliases: ["Fase J5", "J5", "canal-residente", "ShardChannel", "ShardMatmul"]
tags: [implementacao/fase/08, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-08-inferencia]]"
codigos-antigos: ["J5"]
sistema: ["[[inference-engine]]", "[[canal-de-fatias]]", "[[adr-009-canal-residente-de-fatias]]"]
evidencia: ["code/tests/phase-j-shards.sh", "code/crates/engine/src/shard.rs"]
---
# F08 — Canal residente de fatias

- [ ] Superado

> [!note] Contexto
> O forward de [[f08-engine-local]] roda em CPU num nó só. A fila de trabalho do cluster não serve
> para um decode, que encadeia centenas de multiplicações por token. Esta fatia põe os pesos das
> projeções **nos workers**, em fatias residentes, e leva só as ativações pela rede, por um canal
> aberto e de empurrar. A decisão está em [[adr-009-canal-residente-de-fatias]]; o protocolo, em
> [[canal-de-fatias]].

## Objetivo

Executar as projeções de camada de GPT-2 e SmolLM2 nos workers, com resultado idêntico ao local,
tolerância à queda de um worker, e sem que os pesos trafeguem a cada chamada.

## Motivação

**Fila de trabalho (medido).** Latência de `POST /v1/embeddings` (grafo diamante: 4 unidades, 3
níveis de dependência) com 2 workers em contêineres na mesma rede Docker: **54 a 160 ms por
requisição**, em degraus de ~50 ms (o gateway consulta o trace a cada 50 ms). A fila é de **puxar**:
o worker pergunta por trabalho (`WORK_POLL_IDLE_MS = 200` quando ocioso), o resultado volta por
`SubmitWorkResult` e o cliente descobre por polling. Piso por operação dependente: algumas dezenas
de ms. Com ~210 matmuls dependentes por token, isso dá **4 a 10 s por token** contra ~105 ms local
(40 a 100× pior). A fila foi feita para trabalho em lote independente, não para o caminho crítico de
um decode.

**Enviar pesos (inviável).** 28 MB por camada em f64 a cada chamada.

**Pesos residentes (o caminho).** Cada worker guarda uma fatia de colunas ou linhas e só as ativações
trafegam (~4,6 KB). Mesmo assim são ~210 idas e voltas por token: com RTT de 0,3–1 ms em LAN isso
soma 60–210 ms por token, mais que o custo local inteiro; em Wi-Fi, segundos. Para um modelo deste
tamanho distribuir **não ganha tempo** numa LAN; o ganho aparece quando o modelo não cabe num nó ou
há lote e prefill grande. Por isso o canal é um caminho de **capacidade**, não de velocidade, e a
medição abaixo confirma.

## O que foi feito

```text
gateway --ShardMatmul(x, ops)--> controller --ShardCompute--> worker (stream residente)
        <------ saídas ---------            <--ShardResult---
```

- **Divisão.** Os pesos de cada projeção (q, k, v, o, gate, up, down de cada camada) ficam nos
  workers, cortados por linhas de saída (`[out, in]`, bandas iguais, uma por nó). O gateway guarda
  só embeddings, normas e a projeção de saída. Para cada projeção manda a ativação aos nós e junta as
  saídas por colunas. Projeções com a mesma entrada (q, k, v; gate, up) vão numa chamada só: 4 idas
  e voltas por camada.
- **Canal.** `ShardChannel` (stream bidirecional) é aberto pelo worker, que não precisa de porta. O
  nó vem no metadata `x-ai-cluster-node-id` e a credencial no cabeçalho de sempre. O controller exige
  nó **registrado, aprovado, vivo e com a credencial exata**; o token de operador não vale como
  atalho. Sessão nova substitui a antiga e falha o que esperava nela; `revoke` e `leave` derrubam a
  sessão. Keepalive HTTP/2 de 10 s nos dois lados detecta conexão meio-aberta.
- **Pesos não trafegam.** O worker lê só as linhas da sua fatia do `model.safetensors` do próprio
  disco (`read_rows`: cabeçalho + `seek`), na primeira vez que a fatia é pedida, e a mantém residente
  (`ShardStore`). A chave da fatia é `(subdir, tensor, linhas)`. `AI_CLUSTER_MODEL_DIR` liga o canal
  no worker; `AI_CLUSTER_SHARD_CACHE_MB` (padrão 2048) limita a memória das fatias. O subdiretório só
  aceita componentes simples, é canonicalizado e precisa continuar sob a raiz; o nome do arquivo é
  fixo.
- **Relay.** O controller só repassa: `op_id` próprio, uma resposta só vale para a operação enviada
  àquele nó, fila de 64 quadros e 64 operações em voo por nó. `ShardMatmul` é limitado a 64
  operações e 2 Mi valores de entrada, com prazo de 60 s no controller e operações descartadas ao
  falhar uma irmã.
- **Gateway.** `AI_CLUSTER_SMOLLM2_SHARDS=worker-1,worker-2` (e `AI_CLUSTER_SMOLLM2_SUBDIR`, padrão
  `smollm2`) liga o modo remoto para o modelo base; `AI_CLUSTER_SMOLLM2_INSTRUCT_SHARDS` (subdiretório
  padrão `smollm2-instruct`) faz o mesmo para o instruct; `AI_CLUSTER_GPT2_SHARDS` e
  `AI_CLUSTER_GPT2_SUBDIR` para o GPT-2. O log `remoto:` mostra chamadas, tempo nas chamadas e o
  cálculo dos workers.
- **GPT-2 pelo mesmo canal.** `Gpt2::load_remote` corta qkv, c_proj, c_fc e c_proj do MLP em
  **colunas de saída** (os pesos do `Conv1D` são `[in, out]`): o worker lê a faixa de colunas com um
  `seek` por linha (`read_band`) e a fatia já fica como `[in, faixa]`, sem transposta. O viés é
  pequeno e fica no gateway. São 4 idas e voltas por bloco (48 por token, contra 120 da SmolLM2),
  porque nada aqui compartilha entrada. A mensagem `ShardOp`/`ShardCompute` ganhou `by_columns`.
- **Papel de inferência.** `AI_CLUSTER_INFERENCE_TOKEN` (opcional, diferente do de join e do de
  operador) autentica **só `ShardMatmul`**. O controller embrulha cada serviço num `InferenceGate`:
  quem apresenta esse token e chama qualquer outro caminho (listar nós, aprovar, enfileirar, abrir o
  canal de um worker, sondar enlace) recebe `PermissionDenied`. Um RPC novo fica fora do alcance do
  token até alguém liberá-lo. O gateway só com esse token roda os modelos nos workers sem poder
  aprovar nós nem enfileirar trabalho; `/v1/embeddings` fica desligado (503) nele. Com os dois tokens
  os modelos usam o mais estreito.
- **Allowlist de projeções.** O worker só lê projeções de camada (GPT-2 e Llama). Embeddings e
  normas são recusados.
- **Cache com despejo LRU.** Quando o cache de fatias enche, o worker despeja a fatia menos usada e
  carrega de novo sob demanda, em vez de recusar o pedido.
- **Flag `invalid`.** O worker marca `invalid` quando recusa o próprio pedido (tensor fora da lista,
  entrada incoerente). Isso falha com 503 sem acusar o nó nem pôr o nó de lado.
- **Prazo e cancelamento dentro do token.** O prazo de 30 s e o cancelamento (cliente que sai) valem
  também dentro de um `ShardMatmul`: a chamada em voo é abandonada e o controller esquece as operações.

### Tolerância a falha de worker

Qualquer worker computa qualquer faixa (lê do próprio disco), então mover uma faixa não copia nada.
Quando um `ShardMatmul` falha por um nó (sem canal, indisponível, prazo, erro de cálculo), o controller
devolve o erro com o cabeçalho `x-ai-cluster-failed-node`; o `GrpcExecutor` do gateway põe esse nó de
lado por 30 s (`Roster`), entrega as faixas dele aos vivos e **refaz a chamada**. A chamada é pura; o
KV cache está no gateway, então a geração continua do mesmo ponto. Cada tentativa tira mais um nó, então
o laço termina; sem nenhum vivo o pedido dá 503.

- Um erro do nó (modelo ausente no disco dele, leitura) acusa o nó, e o gateway refaz nos outros.
- Queda, canal fechado e falta de resposta contam como falha do nó. Se todos caem, o gateway tenta
  todos de novo em vez de recusar.

## Decisões

- **Pesos residentes, nunca por chamada.** Única forma de o canal ser viável em rede.
- **Chave de fatia sem versão.** `(subdir, tensor, linhas)` basta enquanto o diretório do modelo não
  muda sob o cluster. Isso é também o que deixa os pesos sem versão (ver "Limites").
- **Canal de empurrar, aberto pelo worker.** Nó atrás de NAT não precisa de porta.
- **Token de inferência estreito.** Vale só para o que o gateway precisa; os outros tokens não mudam.
- **Refazer é seguro porque o cálculo é puro.** Por isso a reatribuição não precisa de checkpoint.

## Evidência

- `bash code/tests/phase-j-shards.sh [gpt2|smollm2]` sobe controller, 2 workers e gateway:
  - mesma continuação do local;
  - 503 (sem pendurar) quando um worker some;
  - o gateway só com o token de inferência roda os modelos e não lista nós;
  - um worker morto no meio da geração: a continuação sai idêntica e o log mostra as chamadas
    refeitas.
- `sharded_projections_match_the_local_forward` (`code/crates/engine`): forward fatiado em 3 nós,
  lendo os arquivos, dá logits **idênticos** ao local (GPT-2 e SmolLM2).

### Custo medido (contêineres numa máquina, rede Docker local)

SmolLM2-135M, 2 workers, 120 chamadas por token, depois do aquecimento: **~2,5 ms por chamada**, dos
quais ~1,1 ms são o cálculo do worker (o resto é gateway → controller → worker e volta). Isso dá
~300 ms por token contra ~105 ms local: **cerca de 3× mais lento**, contra os 40–100× da fila de
trabalho. Os dois workers dividem os mesmos 4 núcleos; numa rede real o RTT soma a cada chamada.

## Limites aceitos

- **Token de inferência escolhe nó, subdiretório, tensor e faixa.** A contenção na raiz de modelos é
  do worker. Um token vazado pode forçar o despejo (LRU) das fatias residentes e gerar leituras com
  `by_columns` (um `seek` por linha). O controller aceita 8 `ShardMatmul` simultâneos
  (`ResourceExhausted` acima disso), e o nó, o subdiretório e a faixa continuam à escolha do token.
- **Sem quota por token** nem por cliente: é o papel de [[fase-11-request-manager]].
- **Divisão só por linhas de saída (SmolLM2) ou colunas (GPT-2), entrada replicada.** Sem all-reduce
  e sem pipeline entre camadas: são o escopo de [[fase-12-model-runtime]].
- **Tolerância a falha só por reatribuição.** Sem réplica de fatias pré-carregada, o nó que herda
  uma faixa a lê do disco na primeira vez (segundos de aquecimento). Sem rebalanceamento proativo
  quando um nó entra: a faixa volta ao dono quando o castigo de 30 s acaba.
- **Quarentena e drain não derrubam a sessão** (só `revoke` e `leave`); operações a nós
  indisponíveis são recusadas pelo `ShardMatmul`.
- **Com muitos nós e poucos vivos**, as faixas movidas podem passar de 64 operações em voo por nó
  (503).
- **Pesos sem versão nem checksum.** O worker lê `model.safetensors` do caminho fixo; não há
  manifest. Migração: [[model-manager]] e [[fase-12-model-runtime]].
- **Modelo e placement no gateway.** O gateway escolhe os nós por variável de ambiente
  (`*_SHARDS`), não pelo controller. Alvo: placement do controller, ver [[fase-12-model-runtime]].
- **Aquecimento.** A primeira chamada de cada fatia é lenta.

> [!info] O que já existe
> Canal residente com `ShardChannel`, relay com limites, GPT-2 e SmolLM2 (base e instruct) fatiados
> nos workers, reatribuição de faixas em falha de nó, papel de inferência separado, prazo e
> cancelamento, cache LRU e flag `invalid`, em `code/crates/controller/src/shards.rs`,
> `code/crates/worker/src/shards.rs` e `code/crates/engine/src/shard.rs`.

## Relacionados

- Fase: [[fase-08-inferencia]] · Sistema: [[inference-engine]], [[canal-de-fatias]]
- Decisão: [[adr-009-canal-residente-de-fatias]], [[adr-010-papeis-de-credencial]]
- Irmãos: [[f08-gateway-e-api]], [[f08-chat-completions]], [[f08-engine-local]]
