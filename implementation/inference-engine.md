# Inference Engine (Fase J2)

Primeira fatia do Inference Engine: o **forward do GPT-2 small (124M)** em CPU, num crate à parte
(`code/crates/engine`), sem rede e sem scheduler. Ele roda sobre o `Matrix` do `common`, o mesmo
que os workers multiplicam, e toda camada densa passa por `Matrix::multiply_with_threads`. Esse é
o ponto em que uma fatia futura troca o produto local por um grafo distribuído.

## Primeira família de LLM

GPT-2 small, em float32 (safetensors). Escolhida por ter a arquitetura transformer mais simples
(embeddings absolutos, LayerNorm, MLP com GELU, sem RoPE nem GQA), caber em qualquer nó (~500 MB)
e ter implementação de referência amplamente usada. A segunda família será a SmolLM2-135M
(arquitetura Llama: RoPE, GQA, RMSNorm).

## O que existe

- `safetensors.rs`: leitor mínimo (só `F32`; valida dtype, forma e intervalos de bytes).
- `tokenizer.rs`: BPE em nível de byte do GPT-2. O pré-tokenizador é a regex do GPT-2 escrita à
  mão (ela usa lookahead). `char::is_alphabetic`/`is_numeric` fazem o papel de `\p{L}`/`\p{N}`.
- `gpt2.rs`: `Gpt2::load(dir)` e `next_token_logits(&[u32])`; atenção multi-head causal,
  `gelu_new`, saída amarrada ao embedding.

## Verificação de paridade

`crates/engine/reference/gpt2_reference.py` roda o modelo com `transformers` e grava
`tests/fixtures/gpt2-reference.json`: ids do tokenizador para 6 textos (contrações, espaços,
quebras de linha, acentos, CJK, dígitos) e os 10 maiores logits do último token de 3 prompts.
`tests/gpt2_parity.rs` exige o mesmo token mais provável e logits dentro de 1e-3. Os pesos não
ficam no repositório (`code/models/` é ignorado); sem `AI_CLUSTER_GPT2_DIR` o teste avisa e passa
sem verificar.

```
# pesos (de openai-community/gpt2): model.safetensors, vocab.json, merges.txt, config.json
AI_CLUSTER_GPT2_DIR=code/models/gpt2 cargo test --release -p ai-cluster-engine
```

## Limites

- Só prefill: sem KV cache, sem geração (laço de amostragem), sem streaming. Cada chamada
  recalcula o contexto inteiro.
- CPU, `Matrix` em f64 (~1 GB de RAM para o GPT-2 small); sem quantização.
- Local: nada disso roda ainda nos workers. O gateway continua servindo o modelo sintético.
- O custo de distribuir o forward não foi medido; para um modelo pequeno a latência de rede pode
  superar o ganho.
