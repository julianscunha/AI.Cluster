---
title: "Modelos suportados e homologados"
aliases: ["Modelos", "Catálogo de modelos"]
tags: [guia, modelos]
type: guia
---

# Modelos suportados e homologados

Referência dos modelos de pesos abertos que o AI Cluster tem como alvo, do mais simples ao mais avançado. O nome do modelo é o link de download. Volte ao [[guia]] para o índice.

**Estado real hoje:** só os modelos com ✅ rodam e foram testados no cluster (GPT-2 e SmolLM2, motor próprio). Os demais são o alvo do plano do [[adr-022-fluxo-de-modelos-e-motor-hibrido]]: entram em ✅ quando passarem no teste de ponta a ponta. O cluster não tem teto de tamanho: um modelo cabe se a soma da RAM dos workers comportar.

> **Legenda.** ✅ homologado no cluster · 🔒 exige login e aceite de licença no Hugging Face · ⛔ não suportado ainda
> **Nv** (por parâmetros): 1 até 1B · 2 de 1B a 4B · 3 de 7B a 15B · 4 de 30B a 50B · 5 acima de 60B
> **RAM:** GB do modelo comprimido em 4 bits (Q4), cerca de 0,6 GB por bilhão de parâmetros, só os pesos; o contexto soma mais. **Ctx:** mil tokens.
> **Mod** (entrada→saída): 📝 texto · 🖼️ imagem · 🔊 áudio
> **Notas (0–100):** **Rac** raciocínio (MMLU-Pro; `*` = MMLU) · **Cód** código (HumanEval) · **Mat** matemática (GSM8K; `†` = MATH, `‡` = MATH-500) · **Ins** instruções (IFEval) · **Fer** chamada de ferramentas (BFCL). `—` = a ficha oficial não publica.
> **Idi:** `pt` = português listado na ficha · **Esp:** 🧮 matemática · 👁️ visão

## Texto e visão

| Nv | Modelo | Par. | RAM | Ctx | Mod | Rac | Cód | Mat | Ins | Fer | Idi | Esp | ✅ |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | [GPT-2](https://huggingface.co/openai-community/gpt2) | 124M | 0,1 | 1 | 📝→📝 | — | — | — | — | — | | | ✅ |
| 1 | [SmolLM2 135M](https://huggingface.co/bartowski/SmolLM2-135M-Instruct-GGUF) | 135M | 0,1 | — | 📝→📝 | 29,3* | — | 1,4 | 29,9 | — | | | ✅ |
| 1 | [SmolLM2 360M](https://huggingface.co/bartowski/SmolLM2-360M-Instruct-GGUF) | 360M | 0,2 | — | 📝→📝 | 32,8* | — | 7,4 | 41,0 | — | | | |
| 1 | [Qwen2.5 0.5B](https://huggingface.co/bartowski/Qwen2.5-0.5B-Instruct-GGUF) | 0,5B | 0,3 | 32 | 📝→📝 | 15,0 | 35,4 | 49,6 | 27,9 | — | pt | | |
| 2 | [Llama 3.2 1B](https://huggingface.co/bartowski/Llama-3.2-1B-Instruct-GGUF) 🔒 | 1,2B | 0,7 | 128 | 📝→📝 | 49,3* | — | 44,4 | 59,5 | 25,7 | pt | | |
| 2 | [Qwen2.5 1.5B](https://huggingface.co/bartowski/Qwen2.5-1.5B-Instruct-GGUF) | 1,5B | 0,9 | 32 | 📝→📝 | 32,4 | 61,6 | 73,2 | 42,5 | — | pt | | |
| 2 | [SmolLM2 1.7B](https://huggingface.co/bartowski/SmolLM2-1.7B-Instruct-GGUF) | 1,7B | 1,0 | — | 📝→📝 | 19,3 | — | 48,2 | 56,7 | 27 | | | |
| 2 | [Gemma 2 2B](https://huggingface.co/bartowski/gemma-2-2b-it-GGUF) 🔒 | 2,6B | 1,6 | — | 📝→📝 | 51,3* | 17,7 | 23,9 | — | — | | | |
| 2 | [Qwen2.5 3B](https://huggingface.co/bartowski/Qwen2.5-3B-Instruct-GGUF) | 3,1B | 1,9 | 32 | 📝→📝 | 43,7 | 74,4 | 86,7 | 58,2 | — | pt | | |
| 2 | [Llama 3.2 3B](https://huggingface.co/bartowski/Llama-3.2-3B-Instruct-GGUF) 🔒 | 3,2B | 1,9 | 128 | 📝→📝 | 63,4* | — | 77,7 | 77,4 | 67,0 | pt | | |
| 2 | [Phi 3.5 mini](https://huggingface.co/bartowski/Phi-3.5-mini-instruct-GGUF) | 3,8B | 2,3 | 128 | 📝→📝 | 47,4 | 62,8 | 86,2 | — | — | pt | | |
| 3 | [Mistral 7B v0.3](https://huggingface.co/bartowski/Mistral-7B-Instruct-v0.3-GGUF) | 7,2B | 4,3 | — | 📝→📝 | — | — | — | — | — | | | |
| 3 | [Qwen2.5 7B](https://huggingface.co/Qwen/Qwen2.5-7B-Instruct-GGUF) | 7,6B | 4,6 | 128 | 📝→📝 | — | — | — | — | — | pt | | |
| 3 | [DeepSeek-R1 Distill Qwen 7B](https://huggingface.co/bartowski/DeepSeek-R1-Distill-Qwen-7B-GGUF) | 7,6B | 4,6 | — | 📝→📝 | — | — | 92,8‡ | — | — | | 🧮 | |
| 3 | [Qwen2-VL 7B](https://huggingface.co/bartowski/Qwen2-VL-7B-Instruct-GGUF) | 8B | 4,8 | — | 🖼️📝→📝 | — | — | — | — | — | | 👁️ | |
| 3 | [Llama 3.1 8B](https://huggingface.co/bartowski/Meta-Llama-3.1-8B-Instruct-GGUF) 🔒 | 8B | 4,8 | 128 | 📝→📝 | 48,3 | 72,6 | 84,5 | 80,4 | 76,1 | pt | | |
| 3 | [Gemma 2 9B](https://huggingface.co/bartowski/gemma-2-9b-it-GGUF) 🔒 | 9B | 5,4 | — | 📝→📝 | 71,3* | 40,2 | 68,6 | — | — | | | |
| 3 | [Llama 3.2 Vision 11B](https://huggingface.co/meta-llama/Llama-3.2-11B-Vision-Instruct) 🔒 | 10,6B | 6,4 | 128 | 🖼️📝→📝 | 33,0 | — | 51,9† | — | — | pt | 👁️ | |
| 3 | [Qwen2.5 14B](https://huggingface.co/Qwen/Qwen2.5-14B-Instruct-GGUF) | 14,7B | 8,8 | 128 | 📝→📝 | — | — | — | — | — | pt | | |
| 4 | [Qwen2.5 32B](https://huggingface.co/Qwen/Qwen2.5-32B-Instruct-GGUF) | 32,5B | 19,5 | 128 | 📝→📝 | — | — | — | — | — | pt | | |
| 4 | [Mixtral 8x7B](https://huggingface.co/TheBloke/Mixtral-8x7B-Instruct-v0.1-GGUF) | 46,7B | 28 | — | 📝→📝 | 43,3 | — | — | — | — | | | |
| 5 | [Llama 3.3 70B](https://huggingface.co/bartowski/Llama-3.3-70B-Instruct-GGUF) 🔒 | 70B | 42 | 128 | 📝→📝 | 68,9 | 88,4 | 77,0† | 92,1 | 77,3 | pt | | |
| 5 | [DeepSeek-R1 Distill Llama 70B](https://huggingface.co/bartowski/DeepSeek-R1-Distill-Llama-70B-GGUF) | 70,6B | 42 | — | 📝→📝 | — | — | 94,5‡ | — | — | | 🧮 | |
| 5 | [Qwen2.5 72B](https://huggingface.co/Qwen/Qwen2.5-72B-Instruct-GGUF) | 72,7B | 44 | 128 | 📝→📝 | — | — | — | — | — | pt | | |

## Imagem e áudio (⛔ não suportado ainda)

Exigem outros motores (não texto→texto) e ainda não estão no plano de execução.

| Modelo | Par. | RAM | Mod | Motor necessário |
|---|---|---|---|---|
| [Stable Diffusion XL](https://huggingface.co/stabilityai/stable-diffusion-xl-base-1.0) | 3,5B | 7 | 📝→🖼️ | stable-diffusion.cpp ⛔ |
| [Whisper large-v3](https://huggingface.co/openai/whisper-large-v3) | 1,5B | 3 | 🔊→📝 | whisper.cpp ⛔ |

## Como ler as notas

- **Coletadas em 2026-10-28** das fichas oficiais no Hugging Face e, no caso do Qwen2.5 de 0,5B a 3B, do blog oficial do Qwen. Os valores vêm de uma leitura automática das páginas e **não foram conferidos um a um**; confirme na ficha antes de decidir algo importante.
- **Benchmarks diferentes:** cada autor publica os seus. Só compare números com a mesma marca (`*`, `†`, `‡` ou sem marca). `—` não significa nota zero, e sim que a ficha oficial não publica.
- **Gemma 2:** as notas são do modelo base (pré-treinado), não da versão `it`.
- **SmolLM2 e Llama:** a ficha do SmolLM2 traz o MMLU em formato cloze, não comparável ao MMLU-Pro. O Llama 3.2 1B e 3B só publicam o MMLU.
- **RAM:** é só dos pesos em Q4. O cache de contexto e a margem entram no cálculo do OK/NOK do controller ([[adr-022-fluxo-de-modelos-e-motor-hibrido]]).
- **Qwen2.5 7B ou maior:** o contexto de 128K usa a técnica YaRN; o padrão é 32K.
- **Licenças:** confira a licença de cada ficha antes de uso comercial (por exemplo, o Qwen2.5 3B usa a `qwen-research`).
