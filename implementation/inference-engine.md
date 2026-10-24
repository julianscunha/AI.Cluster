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
