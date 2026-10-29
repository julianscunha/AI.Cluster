---
title: "Inference Engine"
aliases: ["Motor de inferência", "Inference Engine"]
tags: [sistema/subsistema/ia]
type: sistema/subsistema
---

# Inference Engine

O Inference Engine executa a matemática do modelo: tokenização, prefill, decode autoregressivo com KV cache e amostragem. Não decide a topologia do cluster. Quem escolhe onde cada parte roda são o [[execution-planner]] e o [[scheduler]], e o estado persistido é do [[state-manager]]. Voltar para [[ia]].

Hoje o motor é uma biblioteca em CPU, carregada pelo gateway, e pode enviar as projeções lineares para workers.

## Modelos suportados

| Família | Modelos | Pesos | Notas |
|---|---|---|---|
| GPT-2 | GPT-2 small (124M) | float32 (safetensors) | Atenção multi-head causal, LayerNorm, GELU, saída amarrada ao embedding. |
| Llama | SmolLM2-135M, SmolLM2-135M-Instruct | BF16 | RoPE, atenção com grupos de consulta (GQA), RMSNorm, SwiGLU. Saída amarrada aos embeddings ou separada (`lm_head.weight`). |

**O que o motor Llama aceita na carga:** `hidden_act` `silu`; `tie_word_embeddings` verdadeiro (saída = embeddings) ou falso/ausente (exige o tensor `lm_head.weight` com a forma dos embeddings); `rope_scaling` ausente, `default`, `linear` (divide as frequências pelo fator) ou `llama3` (Llama 3.1 em diante: fator, `low_freq_factor`, `high_freq_factor` e `original_max_position_embeddings`); grupos de consulta; contexto até 131072.

**Arquiteturas aceitas além de GPT-2 e Llama:** Qwen2 (viés de q/k/v somado antes do rope, detectado pelos tensores `q_proj.bias`, `k_proj.bias` e `v_proj.bias`, porque o Qwen2 não declara `attention_bias`) e janela deslizante uniforme (Mistral: `sliding_window` com `use_sliding_window` ausente ou verdadeiro; cada token vê só os últimos `window`). O tokenizador usa a expressão regular do Qwen2 quando o `tokenizer.json` a traz (letras com um caractere de abertura opcional, dígitos soltos, pontuação com quebras de linha), conferida contra o `transformers` com código, CRLF, tabulações, contrações e acentos.

**Paridade do Qwen2.5-0.5B:** logits do último token dentro de 1e-3 do `transformers`, ids do tokenizador idênticos e geração com cache igual ao recálculo, medidos com as **4 primeiras camadas** do modelo (`AI_CLUSTER_QWEN2_DIR`, fixture `qwen2-reference.json`). O modelo inteiro não cabe: o motor guarda os pesos em `f64` (cerca de 4 GB para 0,5B, mais que a RAM de um Docker de 4 GB) e a tabela de embeddings sozinha ocupa 1,1 GB. Isso vale para qualquer modelo grande; o caminho de produção é o llama.cpp.

**O que ele recusa, com a razão no erro:** viés em `o_proj`, `attention_bias` ou `mlp_bias` declarados, outras ativações além de `silu`, `rope_scaling` de tipo `yarn`, `dynamic` ou `longrope`, janela deslizante por camada (`layer_types`, Gemma 2), MoE (Mixtral, Qwen-MoE: os especialistas não cabem no hardware doméstico e o roteamento não existe no motor) e qualquer arquitetura que não seja GPT-2, Llama ou Qwen2. Gemma exige ainda normalização com +1 e escala de embedding, que o motor não tem. A saída não amarrada e o `rope_scaling` têm testes unitários (carga de um modelo minúsculo, frequências de referência do Llama 3.1 8B); a janela e o viés têm testes de propriedade com um modelo minúsculo. A paridade com o `transformers` vale para GPT-2, SmolLM2 e Qwen2.5-0.5B (4 camadas); um Llama 3.x de 8B não foi comparado por falta de RAM para os pesos.

O tokenizador é BPE em nível de byte. No modelo SmolLM2, cada dígito vira uma peça separada antes da regex. Trechos sem separador acima de 512 bytes são recusados, para proteger o tempo de tokenização.

## Geração

```text
Tokenização -> Prefill (cria o KV cache) -> loop de decode -> saída
```

- **Prefill** processa o prompt inteiro e cria o KV cache, que guarda K e V de cada camada.
- **Decode** gera um token por vez, reutilizando o cache.
- A decodificação é gulosa: o token de maior logit é escolhido. Não há amostragem, `temperature`, `top_p` nem `seed`.
- A geração para no token de fim de texto (`50256` no GPT-2, `<|im_end|>` no instruct) ou no limite de tokens.
- O cancelamento é verificado a cada token.
- O KV cache é um bloco contíguo por geração, uma posição por token. Não é compartilhado entre gerações.

## Projeções distribuídas

Só as projeções lineares de cada camada podem ser executadas em workers. Embeddings, normas e a projeção de saída (no SmolLM2) ficam no gateway.

- **Ativação de entrada replicada.** O gateway envia a mesma entrada a todos os nós da lista. Cada nó multiplica pela sua faixa.
- **Divisão.** No SmolLM2 a faixa é de linhas de saída. No GPT-2 é de colunas de saída, porque os pesos do `Conv1D` estão em `[entrada, saída]`. Não há all-reduce nem pipeline entre camadas.
- **Pesos residentes.** Cada worker lê do próprio disco só a sua faixa, na primeira vez que ela é pedida, e a mantém em memória. Pesos não trafegam pela rede a cada chamada.
- **Cache de faixas.** Quando o cache enche, a faixa menos usada sai e é lida de novo sob demanda. O tamanho é `AI_CLUSTER_SHARD_CACHE_MB` (padrão 2048 MB).
- **Quais projeções.** Só tensores de projeção de camada, em lista fechada. Embeddings e normas são recusados pelo worker.
- **Canal.** Cada worker abre, por iniciativa própria, um canal bidirecional com o controller. Não precisa de porta de entrada. O controller só aceita o canal de um nó registrado, aprovado, vivo e com a credencial dele. O controller repassa as operações, e cada resposta vale só para a operação enviada àquele nó.
- **Limites do repasse.** Até 64 operações por chamada, 2 Mi valores de entrada, 8 chamadas simultâneas no controller, 64 operações em voo por nó, e 8 s de prazo por chamada.
- **Recusa do próprio worker.** Se o worker recusa um pedido por erro dele (tensor fora da lista, entrada incoerente), a resposta sinaliza `invalid`. O gateway responde 503 sem tirar o nó de serviço.

### Falha de worker

Qualquer worker calcula qualquer faixa, porque lê do próprio disco. Quando um nó falha, o controller devolve o identificador do nó na resposta de erro. O gateway deixa esse nó de fora por 30 s, redistribui as faixas entre os nós vivos e refaz a chamada. A chamada é pura e o KV cache fica no gateway, então a geração continua do mesmo ponto. Se nenhum nó responde, o pedido recebe 503.

Quando o prazo de 30 s termina, as faixas voltam ao nó dono. Não há rebalanceamento proativo quando um nó entra.

## Precisão e determinismo

- O produto matricial usa `f64` no CPU. Isso é simples e exato, ao custo de memória.
- Critério de correção: o motor precisa escolher o mesmo token mais provável que a referência `transformers`, com logits dentro de 1e-3, para os prompts de referência. Esse teste de paridade acompanha o código.
- Mesmo prompt e mesmos pesos dão a mesma saída, e o KV cache dá o mesmo resultado que recalcular o prompt inteiro.

## Desempenho

- No CPU, o decode é limitado pelo produto de uma linha por vez. A divisão por linhas de saída, usada para a projeção de vocabulário, usa threads.
- Cada projeção remota custa uma ida e volta de rede. Um token da SmolLM2 faz quatro dessas por camada. Com a rede real, esse custo tende a superar o cálculo local.
- Por isso a distribuição é um caminho de **capacidade**: serve para modelos que não cabem em um nó, ou para lotes e prefill grandes. Não é um caminho de velocidade em rede local.

## Limites conhecidos

- **O motor roda dentro do gateway.** Não é um serviço separado, e o gateway carrega o modelo na própria memória.
- **Sem quantização.** Os pesos vão para `f64`. O GPT-2 small ocupa cerca de 1 GB só de matrizes.
- **Só decodificação gulosa.** Não há amostragem, temperatura nem seed.
- **Uma geração de cada vez,** sem batching e sem KV cache paginado.
- **Divisão de uma só dimensão.** Sem pipeline, sem paralelismo de tensor com all-reduce e sem decodificação especulativa.
- **Sem checkpoint de geração.** Se o gateway cai, a geração em curso é perdida. A tolerância existe só para falha de worker.
- **Sem validação numérica.** Não há checagem de NaN nem de Inf na saída.
- **Sem versão nas faixas.** A chave de uma faixa é (subdiretório, tensor, linhas). Trocar os pesos com o cluster rodando exige reiniciar os workers. Ver [[model-manager]].
- **Cálculo local de uma linha não usa pool de threads.** Cada chamada cria threads novas, o que custa tempo em matrizes pequenas.

> Planejado: motor como serviço separado do gateway (Model Runtime), com variantes Q8 e Q4 e compute INT8.
> Planejado: amostragem com `temperature`, `top_p`, `stop` e `seed`.
> Planejado: batching contínuo e KV cache paginado.
> Planejado: pipeline, paralelismo de tensor com all-reduce e decodificação especulativa.
> Planejado: checkpoint por epoch e recuperação de geração interrompida pelo [[state-manager]].
> Planejado: validação numérica no caminho de verificação.
> Planejado: backends ARM64, GPU e NPU.

## Relacionados

- Fronteira e rotas: [[inference-api]]. Pesos e manifesto: [[model-manager]].
- Protocolo do canal de fatias: [[canal-de-fatias]]. Decisão: [[adr-009-canal-residente-de-fatias]].
- Papel de inferência e tokens: [[seguranca]].
