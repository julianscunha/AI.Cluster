# Inference Engine (Fase J2)

Primeira fatia do Inference Engine: o **forward do GPT-2 small (124M)** em CPU, num crate à parte
(`code/crates/engine`), sem rede e sem scheduler. Ele roda sobre o `Matrix` do `common`, o mesmo
que os workers multiplicam, e toda camada densa passa por `Matrix::multiply_with_threads`. Esse é
o ponto em que uma fatia futura troca o produto local por um grafo distribuído.

## Primeira família de LLM

GPT-2 small, em float32 (safetensors). Escolhida por ter a arquitetura transformer mais simples
(embeddings absolutos, LayerNorm, MLP com GELU, sem RoPE nem GQA), caber em qualquer nó (~500 MB)
e ter implementação de referência amplamente usada. A segunda família é a SmolLM2-135M
(arquitetura Llama: RoPE, GQA, RMSNorm, SwiGLU; pesos em BF16).

## O que existe

Tudo que o gateway precisa de um modelo está no trait `LanguageModel` (`forward` com `KvCache`,
`generate` guloso com cancelamento); `Gpt2` e `Llama` o implementam.

- `safetensors.rs`: leitor mínimo (`F32` e `BF16`; valida dtype, forma e intervalos de bytes).
- `tokenizer.rs`: BPE em nível de byte do GPT-2. Com `tokenizer.json` presente e `Digits`
  individual (SmolLM2), cada dígito vira uma peça antes da regex. Trechos sem separador acima de
  512 bytes são recusados (o laço de merge é quadrático). Tokens especiais no texto
  (`<|im_start|>`, `<|endoftext|>`) não são reconhecidos: viram bytes comuns. O pré-tokenizador é a regex do GPT-2 escrita à
  mão (ela usa lookahead). `char::is_alphabetic`/`is_numeric` fazem o papel de `\p{L}`/`\p{N}`.
- `llama.rs`: `Llama::load(dir)` para checkpoints Llama com embeddings amarrados, sem bias nem
  `rope_scaling` (outros são recusados). RoPE "rotate half", atenção com grupos de consulta,
  SwiGLU. Os pesos `[out, in]` são transpostos no carregamento.
- `gpt2.rs`: `Gpt2::load(dir)` e `next_token_logits(&[u32])`, `forward` com `KvCache` e `generate`; atenção multi-head causal,
  `gelu_new`, saída amarrada ao embedding.

## Verificação de paridade

`crates/engine/reference/gpt2_reference.py` roda o modelo com `transformers` e grava
`tests/fixtures/gpt2-reference.json`: ids do tokenizador para 6 textos (contrações, espaços,
quebras de linha, acentos, CJK, dígitos) e os 10 maiores logits do último token de 3 prompts.
`tests/gpt2_parity.rs` (e `smollm2_parity.rs`, com `AI_CLUSTER_SMOLLM2_DIR`, fixture gerada por
`smollm2_reference.py`) exige o mesmo token mais provável e logits dentro de 1e-3. Os pesos não
ficam no repositório (`code/models/` é ignorado); sem `AI_CLUSTER_GPT2_DIR` o teste avisa e passa
sem verificar.

```
# pesos (de openai-community/gpt2): model.safetensors, vocab.json, merges.txt, config.json
AI_CLUSTER_GPT2_DIR=code/models/gpt2 cargo test --release -p ai-cluster-engine
```

## Limites

- Geração só gulosa (`generate`, com parada em token), sem amostragem por temperatura/top-p e sem
  streaming. O KV cache (`KvCache`, `forward`) é verificado contra recalcular o prompt inteiro a
  cada passo (`cached_generation_matches_recomputing_the_whole_prompt`).
- CPU, `Matrix` em f64 (~1 GB de RAM para o GPT-2 small); sem quantização.
- Local: nada disso roda ainda nos workers. O gateway continua servindo o modelo sintético.
- O custo de distribuir o forward não foi medido; para um modelo pequeno a latência de rede pode
  superar o ganho.

## Custo local medido (antes de distribuir)

`cargo run --release -p ai-cluster-engine --example bench -- <gpt2|smollm2> <dir>`, num contêiner
de 4 núcleos (Docker no Windows), prompt de 8 tokens:

| modelo | carga | prompt | por token gerado |
|--------|-------|--------|------------------|
| SmolLM2-135M | 3 s | 0,58 s | **156 ms** |
| GPT-2 small | 15 s | 0,34 s | **140 ms** |

- Um matmul `1×576×1536` (gate/up da SmolLM2) leva **0,65 ms** em uma thread. Um token da SmolLM2
  faz cerca de 210 matmuls de camada mais a projeção de saída (49152×576): ~134 M
  multiplicações-soma em 156 ms, ou seja, **cerca de 1 GMAC/s: uma thread só**. O produto divide
  o trabalho por linhas de saída (`multiply_with_threads`); com 1 linha (decodificação) não há o
  que dividir.
- Consequência para distribuir: enviar os pesos a cada chamada é inviável (28 MB por camada em
  f64); eles teriam de ficar residentes nos workers (cada um com uma fatia de colunas), e só as
  ativações (~4,6 KB) trafegariam. Mesmo assim são ~210 idas e voltas por token: com RTT de
  0,3–1 ms em LAN isso soma 60–210 ms por token, mais do que o custo local inteiro; em Wi-Fi,
  segundos. Para um modelo deste tamanho distribuir **não ganha tempo**; o ganho existe quando
  o modelo não cabe num nó ou quando há lote/prefill grande.
- RTT real entre nós não foi medido aqui (o contêiner único não representa uma rede).

### Paralelizar o decode local (resultado)

- A projeção de saída (49152×576 na SmolLM2) é um produto escalar por linha do vocabulário; passou
  a ser dividida em threads (`project_rows`): **156 → ~105 ms por token** na SmolLM2 e
  140 → ~109 ms no GPT-2, com os mesmos logits (a paridade segue passando).
- Dividir por **colunas** o matmul de uma linha das camadas (`Matrix::multiply_with_threads`) foi
  testado e **descartado**: ficou ~2× mais lento (0,45 → 0,85 ms por matmul e ~195 ms por token),
  porque cada chamada cria threads novas (~840 por token) para ~0,5 ms de trabalho. Só valeria com
  um pool de threads persistente; fica registrado como o próximo ganho local, não feito.
- O que sobra (~105 ms) são ~106 M multiplicações-soma das camadas em uma thread só.

### Custo de passar um matmul pela fila de trabalho atual (medido)

Latência de `POST /v1/embeddings` (grafo diamante: 4 unidades, 3 níveis de dependência) com 2
workers em contêineres na mesma rede Docker: **54 a 160 ms por requisição**, em degraus de ~50 ms
(o gateway consulta o trace a cada 50 ms). A fila é de **puxar**: o worker pergunta por trabalho
(`WORK_POLL_IDLE_MS = 200` quando está ocioso), o resultado volta por `SubmitWorkResult` e o
cliente descobre por polling. Piso por operação dependente: algumas dezenas de ms.

Com ~210 matmuls dependentes por token isso daria **4 a 10 s por token** contra ~105 ms local
(40 a 100× pior). A fila foi feita para trabalho em lote independente, não para o caminho
crítico de um decode. Ligar o `Linear` do engine a ela só confirmaria o número.

Caminho que faria sentido: um canal **residente e de empurrar** (stream gRPC bidirecional aberto
pelo worker, com as fatias de pesos já carregadas e quadros `matmul(fatia, ativação)`), com
latência próxima do RTT. É uma superfície de protocolo nova (autenticação, quotas, carga e
descarte de fatias, falha de um worker no meio do token) e fica como decisão de projeto.

## J5: canal residente de fatias

Os pesos de cada projeção (q, k, v, o, gate, up, down de cada camada) ficam **nos workers**, cortados
por linhas de saída (`[out, in]`, bandas iguais, uma por nó). O gateway guarda só embeddings, normas
e a projeção de saída; para cada projeção manda a ativação aos nós e junta as saídas por colunas.
Projeções com a mesma entrada (q, k, v; gate, up) vão numa chamada só: 4 idas e voltas por camada.

```
gateway --ShardMatmul(x, ops)--> controller --ShardCompute--> worker (stream residente)
        <------ saídas ---------            <--ShardResult---
```

- **Canal.** `ShardChannel` (stream bidirecional) é aberto pelo worker, que não precisa de porta.
  O nó vem no metadata `x-ai-cluster-node-id` e a credencial no cabeçalho de sempre. O controller
  exige nó **registrado, aprovado, vivo e com a credencial exata**; o token de operador não vale
  como atalho. Sessão nova substitui a antiga e falha o que esperava nela; `revoke` e `leave`
  derrubam a sessão. Keepalive HTTP/2 de 10 s nos dois lados detecta conexão meio-aberta.
- **Pesos não trafegam.** O worker lê só as linhas da sua fatia do `model.safetensors` do próprio
  disco (`read_rows`: cabeçalho + `seek`), na primeira vez que a fatia é pedida, e a mantém residente
  (`ShardStore`). A chave da fatia é `(subdir, tensor, linhas)`, então não há versão a sincronizar; a
  primeira chamada é lenta (aquecimento). `AI_CLUSTER_MODEL_DIR` liga o canal no worker;
  `AI_CLUSTER_SHARD_CACHE_MB` (2048) limita a memória das fatias. O subdiretório só aceita
  componentes simples, é canonicalizado e precisa continuar sob a raiz; o nome do arquivo é fixo.
- **Relay.** O controller só repassa: `op_id` próprio, uma resposta só vale para a operação enviada
  àquele nó, fila de 64 quadros e 64 operações em voo por nó, `ShardMatmul` limitado a 64 operações e
  2 Mi valores, prazo de 60 s, operações descartadas ao falhar uma irmã.
- **Gateway.** `AI_CLUSTER_SMOLLM2_SHARDS=worker-1,worker-2` (e `AI_CLUSTER_SMOLLM2_SUBDIR`, padrão
  `smollm2`) liga o modo remoto; o log `remoto:` mostra chamadas, tempo nas chamadas e o cálculo
  dos workers.
- **Verificações.** Forward fatiado em 3 nós, lendo os arquivos, dá logits **idênticos** ao local
  (`sharded_projections_match_the_local_forward`); `tests/phase-j-shards.sh` sobe controller, 2
  workers e gateway: mesma continuação do local, e 503 (sem pendurar) quando um worker some.

### GPT-2 pelo mesmo canal

`Gpt2::load_remote` corta qkv, c_proj, c_fc e c_proj do MLP em **colunas de saída** (os pesos do
`Conv1D` são `[in, out]`): o worker lê a faixa de colunas com um `seek` por linha (`read_band`) e a
fatia já fica como `[in, faixa]`, sem transposta. O viés é pequeno e fica no gateway. São 4 idas e
voltas por bloco (48 por token, contra 120 da SmolLM2), porque nada aqui compartilha entrada. A
mensagem `ShardOp`/`ShardCompute` ganhou `by_columns`. `AI_CLUSTER_GPT2_SHARDS`/`_SUBDIR` ligam o
modo; `tests/phase-j-shards.sh gpt2` e `sharded_projections_match_the_local_forward` (GPT-2) provam
logits idênticos e a mesma continuação.

### Custo medido (contêineres numa máquina, rede Docker local)

SmolLM2-135M, 2 workers, 120 chamadas por token, depois do aquecimento: **~2,5 ms por chamada**, dos
quais ~1,1 ms são o cálculo do worker (o resto é gateway → controller → worker e volta). Isso dá
~300 ms por token contra ~105 ms local: **cerca de 3× mais lento**, ao contrário dos 40–100× da fila
de trabalho. Os dois workers dividem os mesmos 4 núcleos; numa rede real o RTT soma a cada chamada.

### Limites

- O gateway tem o papel de operador e comanda a carga de arquivos nos workers: um papel "inferência"
  separado seria o passo seguinte.
- Sem tolerância a falha: se um worker cai no meio de um token, a requisição inteira falha (503).
  Sem réplica de fatias, sem rebalanceamento, sem redistribuir quando entra um nó.
- Quarentena e drain não derrubam a sessão (só `revoke`/`leave`); as operações a nós indisponíveis
  são recusadas pelo `ShardMatmul`.
- Divisão só por linhas de saída, entrada replicada: sem all-reduce, sem pipeline entre camadas.
