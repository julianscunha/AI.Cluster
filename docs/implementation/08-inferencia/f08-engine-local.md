---
title: "F08 — Engine local (GPT-2 e SmolLM2 em CPU)"
aliases: ["inference-engine", "Fase J2", "Fase J4", "J2", "J4"]
tags: [implementacao/fase/08, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-08-inferencia]]"
codigos-antigos: ["J2", "J4"]
sistema: ["[[inference-engine]]"]
evidencia: ["code/crates/engine/tests/gpt2_parity.rs", "code/crates/engine/tests/smollm2_parity.rs", "code/crates/engine/examples/bench.rs"]
---
# F08 — Engine local (GPT-2 e SmolLM2 em CPU)

- [ ] Superado

> [!note] Contexto
> Primeira parte do [[inference-engine]]: o forward de modelos de linguagem em CPU, num crate à
> parte (`code/crates/engine`), sem rede e sem scheduler. Ele roda sobre o `Matrix` do `common`, o
> mesmo que os workers multiplicam, e toda camada densa passa por `Matrix::multiply_with_threads`.
> É o ponto em que uma fatia futura troca o produto local por um grafo distribuído: ver
> [[f08-canal-residente-de-fatias]].

## Objetivo

Um forward de LLM que seja a referência de correção (paridade com `transformers`) e medir o custo
local antes de decidir distribuir qualquer coisa.

## O que foi feito

### Famílias de modelo

- **GPT-2 small (124M)**, float32 (safetensors). Escolhido por ter a arquitetura transformer mais
  simples (embeddings absolutos, LayerNorm, MLP com GELU, sem RoPE nem GQA), caber em qualquer nó
  (~500 MB) e ter implementação de referência amplamente usada (J2).
- **SmolLM2-135M** e **SmolLM2-135M-Instruct**, arquitetura Llama (RoPE, GQA, RMSNorm, SwiGLU;
  pesos em BF16). Entraram como segunda família (J4).

### Componentes

Tudo que o gateway precisa de um modelo está no trait `LanguageModel` (`forward` com `KvCache`,
`generate` guloso com cancelamento). `Gpt2` e `Llama` o implementam.

- `safetensors.rs`: leitor mínimo (`F32` e `BF16`; valida dtype, forma e intervalos de bytes).
- `tokenizer.rs`: BPE em nível de byte do GPT-2. Com `tokenizer.json` presente e `Digits`
  individual (SmolLM2), cada dígito vira uma peça antes da regex. Trechos sem separador acima de
  512 bytes são recusados (o laço de merge é quadrático). Tokens especiais no texto
  (`<|im_start|>`, `<|endoftext|>`) não são reconhecidos como tokens: viram bytes comuns; o
  template de chat os insere por id ([[f08-chat-completions]]). O pré-tokenizador é a regex do GPT-2
  escrita à mão (usa lookahead). `char::is_alphabetic` e `is_numeric` fazem o papel de `\p{L}` e
  `\p{N}`.
- `llama.rs`: `Llama::load(dir)` para checkpoints Llama com embeddings amarrados, sem bias nem
  `rope_scaling` (outros são recusados). RoPE "rotate half", atenção com grupos de consulta,
  SwiGLU. Os pesos `[out, in]` são transpostos no carregamento.
- `gpt2.rs`: `Gpt2::load(dir)` e `next_token_logits(&[u32])`, `forward` com `KvCache` e `generate`:
  atenção multi-head causal, `gelu_new`, saída amarrada ao embedding.
- `KvCache`: o forward incremental. Verificado contra recalcular o prompt inteiro a cada passo
  (`cached_generation_matches_recomputing_the_whole_prompt`).

## Decisões

- **Paridade é o critério de correção.** O engine não é aceito por parecer certo: tem de bater com
  `transformers` nos ids e nos logits.
- **CPU, `Matrix` em f64.** Simples e exato; custa RAM (~1 GB para o GPT-2 small) e não há
  quantização.
- **Geração gulosa só.** Amostragem fica para depois, quando houver uso que peça.

## Verificação de paridade

`code/crates/engine/reference/gpt2_reference.py` roda o modelo com `transformers` e grava
`tests/fixtures/gpt2-reference.json`: ids do tokenizador para 6 textos (contrações, espaços, quebras
de linha, acentos, CJK, dígitos) e os 10 maiores logits do último token de 3 prompts.

- `tests/gpt2_parity.rs` (e `smollm2_parity.rs`, com `AI_CLUSTER_SMOLLM2_DIR`, fixture gerada por
  `smollm2_reference.py`) exige o mesmo token mais provável e logits dentro de 1e-3.
- Os pesos não ficam no repositório (`code/models/` é ignorado). Sem `AI_CLUSTER_GPT2_DIR` o teste
  avisa e passa sem verificar.

```text
# pesos (de openai-community/gpt2): model.safetensors, vocab.json, merges.txt, config.json
AI_CLUSTER_GPT2_DIR=code/models/gpt2 cargo test --release -p ai-cluster-engine
```

## Evidência

- Paridade GPT-2 e SmolLM2: `gpt2_parity.rs`, `smollm2_parity.rs` (acima).
- Paridade do template de chat: [[f08-chat-completions]].
- Custo: `cargo run --release -p ai-cluster-engine --example bench -- <gpt2|smollm2> <dir>` (tabela
  abaixo).

## Custo local medido (antes de distribuir)

Num contêiner de 4 núcleos (Docker no Windows), prompt de 8 tokens:

| modelo | carga | prompt | por token gerado |
|--------|-------|--------|------------------|
| SmolLM2-135M | 3 s | 0,58 s | **156 ms** |
| GPT-2 small | 15 s | 0,34 s | **140 ms** |

- Um matmul `1×576×1536` (gate/up da SmolLM2) leva **0,65 ms** em uma thread. Um token da SmolLM2
  faz cerca de 210 matmuls de camada mais a projeção de saída (49152×576): ~134 M
  multiplicações-soma em 156 ms, ou seja, **cerca de 1 GMAC/s: uma thread só**. O produto divide o
  trabalho por linhas de saída (`multiply_with_threads`); com 1 linha (decodificação) não há o que
  dividir.
- Para distribuir um modelo deste tamanho, o custo de rede por token (210 idas e voltas) supera o
  custo local inteiro. A análise completa está em [[f08-canal-residente-de-fatias]], "Motivação".
- RTT real entre nós não foi medido aqui (o contêiner único não representa uma rede).

### Paralelizar o decode local (resultado)

- A projeção de saída (49152×576 na SmolLM2) é um produto escalar por linha do vocabulário; passou
  a ser dividida em threads (`project_rows`): **156 → ~105 ms por token** na SmolLM2 e
  140 → ~109 ms no GPT-2, com os mesmos logits (a paridade segue passando).
- Dividir por **colunas** o matmul de uma linha das camadas (`Matrix::multiply_with_threads`) foi
  testado e **descartado**: ficou ~2× mais lento (0,45 → 0,85 ms por matmul e ~195 ms por token),
  porque cada chamada cria threads novas (~840 por token) para ~0,5 ms de trabalho. Só valeria com
  um pool de threads persistente; fica como o próximo ganho local, não feito.
- O que sobra (~105 ms) são ~106 M multiplicações-soma das camadas em uma thread só.

## Limites e próximos passos

- **Geração só gulosa**, sem amostragem por temperatura ou top-p. Streaming e chat estão no
  gateway ([[f08-chat-completions]]), não no engine.
- **CPU, f64, ~1 GB de RAM** para o GPT-2 small; sem quantização.
- **Forward local, não distribuído.** As projeções podem rodar nos workers pelo canal residente
  ([[f08-canal-residente-de-fatias]]); o engine sozinho não distribui nada.
- **Pool de threads persistente** para o decode por colunas: ganho local ainda não feito.
- **Modelos suportados são só os dois** (GPT-2 e Llama sem bias nem `rope_scaling`). Outras
  arquiteturas são recusadas na carga.
- **Pesos sem versão:** o engine lê o diretório que recebe, sem manifest nem checksum. Ver
  [[inference-engine]] e [[model-manager]] para o alvo.

> [!info] O que já existe
> Forward do GPT-2 small e da SmolLM2 (base e instruct) em CPU, tokenizador BPE, KV cache,
> paridade com `transformers` e bench local, em `code/crates/engine`.

## Relacionados

- Fase: [[fase-08-inferencia]] · Sistema: [[inference-engine]]
- Irmãos: [[f08-gateway-e-api]], [[f08-chat-completions]], [[f08-canal-residente-de-fatias]]
