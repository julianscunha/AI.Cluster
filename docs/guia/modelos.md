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
> **Nv** (por parâmetros totais): 1 até 1B · 2 de 1B a 4B · 3 de 7B a 15B · 4 de 20B a 50B · 5 de 60B a 100B · 6 acima de 100B (MoE gigantes: só uma parte dos parâmetros calcula por token, mas todos precisam estar na RAM)
> **AA:** Artificial Analysis Intelligence Index v5.0 (2026-10-27), teste atual e difícil; referência fechada na mesma escala: Claude Opus 5.5 = 58 e Claude Sonnet 5.5 = 54. `—` = o AA não pontua o modelo. Os 5 testes à direita (Rac a Fer) são das fichas e estão saturados: separam mal os modelos fortes.
> **RAM:** GB do modelo comprimido em 4 bits (Q4), cerca de 0,6 GB por bilhão de parâmetros, só os pesos; o contexto soma mais. **Ctx:** mil tokens.
> **Mod** (entrada→saída): 📝 texto · 🖼️ imagem · 🔊 áudio
> **Notas (0–100):** **Rac** raciocínio (MMLU-Pro; `*` = MMLU) · **Cód** código (HumanEval) · **Mat** matemática (GSM8K; `†` = MATH, `‡` = MATH-500) · **Ins** instruções (IFEval) · **Fer** chamada de ferramentas (BFCL). `—` = a ficha oficial não publica.
> **Idi:** `pt` = português listado na ficha · **Esp:** 🧮 matemática · 👁️ visão

## Como rodar um modelo (3 comandos)

Com o controller, o gateway e pelo menos um worker no ar (cada worker com o `rpc-server`, veja [[configuracao]]):

```bash
ai-cluster model list                    # o que há na pasta de modelos do controller (GGUF)
ai-cluster model check <id>              # cabe na RAM dos workers? OK, ou NOK com o que falta
ai-cluster model start <id>              # sobe o modelo; mostra a chave de API uma vez
ai-cluster model status                  # estado (CARREGANDO, PRONTO), workers do plano e tokens/s
```

1. Baixe um modelo em **GGUF** e ponha na pasta `AI_CLUSTER_MODELS_DIR` do controller (a mesma pasta
   precisa estar visível ao gateway).
2. O nome pode ser o arquivo (`qwen25-05b-q4.gguf`) ou a **família** (`qwen25-05b`, o nome sem a etiqueta de quantização `-q4`, `-Q4_K_M`, `-q8_0`, `-f16`...). Pela família, `model check` e `model start` escolhem a variante mais precisa (a maior) que cabe na RAM; se nenhuma cabe, mostram o veredito da menor. O GGUF não traz uma versão confiável: a versão é a do nome do arquivo.
3. `model start` confere a soma da RAM dos workers aprovados e vivos. Se não couber, diz o que falta e
   quantos workers acrescentar. Se couber, o controller guarda o pedido, cria a chave de API (só o
   **hash** fica com ele) e o gateway carrega o modelo pelo llama.cpp, dividido com os workers por RPC
   sobre mTLS.
4. Use a chave como `Authorization: Bearer <chave>` em `POST /v1/chat/completions`, com o `model` que
   `GET /v1/models` mostra (o nome do arquivo sem `.gguf`).
5. **Worker novo:** depois de 30 s com a mesma lista de workers, o controller refaz o plano, o gateway
   recarrega o modelo e o `model status` mostra o benchmark novo. `model stop` tira o modelo da API; a
   chave continua valendo no próximo `start`.

Limites de hoje: uma geração por vez, o certificado do túnel não é por nó e os binários do llama.cpp
ainda não vão nas imagens do SO (veja [[pendencias]]).

## Texto e visão

| Nv | Modelo | Par. | RAM | Ctx | Mod | AA | Rac | Cód | Mat | Ins | Fer | Idi | Esp | ✅ |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | [GPT-2](https://huggingface.co/openai-community/gpt2) | 124M | 0,1 | 1 | 📝→📝 | — | — | — | — | — | — | | | ✅ |
| 1 | [SmolLM2 135M](https://huggingface.co/bartowski/SmolLM2-135M-Instruct-GGUF) | 135M | 0,1 | — | 📝→📝 | — | 29,3* | — | 1,4 | 29,9 | — | | | ✅ |
| 1 | [SmolLM2 360M](https://huggingface.co/bartowski/SmolLM2-360M-Instruct-GGUF) | 360M | 0,2 | — | 📝→📝 | — | 32,8* | — | 7,4 | 41,0 | — | | | |
| 1 | [Qwen2.5 0.5B](https://huggingface.co/bartowski/Qwen2.5-0.5B-Instruct-GGUF) | 0,5B | 0,3 | 32 | 📝→📝 | — | 15,0 | 35,4 | 49,6 | 27,9 | — | pt | | |
| 2 | [Llama 3.2 1B](https://huggingface.co/bartowski/Llama-3.2-1B-Instruct-GGUF) 🔒 | 1,2B | 0,7 | 128 | 📝→📝 | — | 49,3* | — | 44,4 | 59,5 | 25,7 | pt | | |
| 2 | [Qwen2.5 1.5B](https://huggingface.co/bartowski/Qwen2.5-1.5B-Instruct-GGUF) | 1,5B | 0,9 | 32 | 📝→📝 | — | 32,4 | 61,6 | 73,2 | 42,5 | — | pt | | |
| 2 | [SmolLM2 1.7B](https://huggingface.co/bartowski/SmolLM2-1.7B-Instruct-GGUF) | 1,7B | 1,0 | — | 📝→📝 | — | 19,3 | — | 48,2 | 56,7 | 27 | | | |
| 2 | [Gemma 2 2B](https://huggingface.co/bartowski/gemma-2-2b-it-GGUF) 🔒 | 2,6B | 1,6 | — | 📝→📝 | — | 51,3* | 17,7 | 23,9 | — | — | | | |
| 2 | [Qwen2.5 3B](https://huggingface.co/bartowski/Qwen2.5-3B-Instruct-GGUF) | 3,1B | 1,9 | 32 | 📝→📝 | — | 43,7 | 74,4 | 86,7 | 58,2 | — | pt | | |
| 2 | [Llama 3.2 3B](https://huggingface.co/bartowski/Llama-3.2-3B-Instruct-GGUF) 🔒 | 3,2B | 1,9 | 128 | 📝→📝 | — | 63,4* | — | 77,7 | 77,4 | 67,0 | pt | | |
| 2 | [Phi 3.5 mini](https://huggingface.co/bartowski/Phi-3.5-mini-instruct-GGUF) | 3,8B | 2,3 | 128 | 📝→📝 | — | 47,4 | 62,8 | 86,2 | — | — | pt | | |
| 3 | [Mistral 7B v0.3](https://huggingface.co/bartowski/Mistral-7B-Instruct-v0.3-GGUF) | 7,2B | 4,3 | — | 📝→📝 | — | — | — | — | — | — | | | |
| 3 | [Qwen2.5 7B](https://huggingface.co/Qwen/Qwen2.5-7B-Instruct-GGUF) | 7,6B | 4,6 | 128 | 📝→📝 | — | — | — | — | — | — | pt | | |
| 3 | [DeepSeek-R1 Distill Qwen 7B](https://huggingface.co/bartowski/DeepSeek-R1-Distill-Qwen-7B-GGUF) | 7,6B | 4,6 | — | 📝→📝 | — | — | — | 92,8‡ | — | — | | 🧮 | |
| 3 | [Qwen2-VL 7B](https://huggingface.co/bartowski/Qwen2-VL-7B-Instruct-GGUF) | 8B | 4,8 | — | 🖼️📝→📝 | — | — | — | — | — | — | | 👁️ | |
| 3 | [Llama 3.1 8B](https://huggingface.co/bartowski/Meta-Llama-3.1-8B-Instruct-GGUF) 🔒 | 8B | 4,8 | 128 | 📝→📝 | — | 48,3 | 72,6 | 84,5 | 80,4 | 76,1 | pt | | |
| 3 | [Gemma 2 9B](https://huggingface.co/bartowski/gemma-2-9b-it-GGUF) 🔒 | 9B | 5,4 | — | 📝→📝 | — | 71,3* | 40,2 | 68,6 | — | — | | | |
| 3 | [Llama 3.2 Vision 11B](https://huggingface.co/meta-llama/Llama-3.2-11B-Vision-Instruct) 🔒 | 10,6B | 6,4 | 128 | 🖼️📝→📝 | — | 33,0 | — | 51,9† | — | — | pt | 👁️ | |
| 3 | [Qwen2.5 14B](https://huggingface.co/Qwen/Qwen2.5-14B-Instruct-GGUF) | 14,7B | 8,8 | 128 | 📝→📝 | — | — | — | — | — | — | pt | | |
| 4 | [Qwen3.8 27B](https://huggingface.co/Qwen/Qwen3.8-27B) | 27B | 16 | 256 | 🖼️📝→📝 | 20 | — | — | — | — | — | | 👁️ | |
| 4 | [Qwen2.5 32B](https://huggingface.co/Qwen/Qwen2.5-32B-Instruct-GGUF) | 32,5B | 19,5 | 128 | 📝→📝 | — | — | — | — | — | — | pt | | |
| 4 | [Mixtral 8x7B](https://huggingface.co/TheBloke/Mixtral-8x7B-Instruct-v0.1-GGUF) | 46,7B | 28 | — | 📝→📝 | — | 43,3 | — | — | — | — | | | |
| 5 | [Llama 3.3 70B](https://huggingface.co/bartowski/Llama-3.3-70B-Instruct-GGUF) 🔒 | 70B | 42 | 128 | 📝→📝 | — | 68,9 | 88,4 | 77,0† | 92,1 | 77,3 | pt | | |
| 5 | [DeepSeek-R1 Distill Llama 70B](https://huggingface.co/bartowski/DeepSeek-R1-Distill-Llama-70B-GGUF) | 70,6B | 42 | — | 📝→📝 | — | — | — | 94,5‡ | — | — | | 🧮 | |
| 5 | [Qwen2.5 72B](https://huggingface.co/Qwen/Qwen2.5-72B-Instruct-GGUF) | 72,7B | 44 | 128 | 📝→📝 | — | — | — | — | — | — | pt | | |
| 6 | [Nemotron 3.5 Super VL](https://huggingface.co/nvidia/NVIDIA-Nemotron-3.5-Super-VL-120B-A12B-BF16) | 120B (12B ativos) | 72 | 1000 | 🖼️📝→📝 | 14 | — | — | — | — | — | | 👁️ | |
| 6 | [K2 Horizon](https://huggingface.co/IFM/K2-Horizon-375B-A23B) | 375B (23B ativos) | 225 | 524 | 📝→📝 | 18 | — | — | — | — | — | | | |
| 6 | [Beam 1.0](https://huggingface.co/reflection/Beam-1.0-501B-A23B) | 501B (23B ativos) | 301 | 1000 | 📝→📝 | 18 | — | — | — | — | — | | | |
| 6 | [DeepSeek V4.1 Flash](https://huggingface.co/deepseek-ai/DeepSeek-V4.1-Flash-1017) | 552B (16B ativos) | 331 | 1000 | 🖼️📝→📝 | 32 | — | — | — | — | — | | | |
| 6 | [Ling 3.1 Flash](https://huggingface.co/inclusionAI/Ling-3.1-flash-1019) | 560B (25B ativos) | 336 | 1000 | 📝→📝 | 29 | — | — | — | — | — | | | |
| 6 | [Step 5 Preview](https://huggingface.co/stepfun-ai/Step-5-Preview) | 600B (27B ativos) | 360 | 1000 | 📝→📝 | 29 | — | — | — | — | — | | | |
| 6 | [GLM-5.3](https://huggingface.co/zai-org/GLM-5.3) | 753B (40B ativos) | 452 | 1000 | 📝→📝 | 32 | — | — | — | — | — | | | |
| 6 | [Muse Spark Open](https://huggingface.co/meta-models/Muse-Spark-Open-816B-A42B) | 816B (42B ativos) | 490 | 1000 | 📝→📝 | 25 | — | — | — | — | — | | | |
| 6 | [Inkling](https://huggingface.co/thinkingmachines/Inkling) | 975B (41B ativos) | 585 | 1000 | 📝→📝 | 12 | — | — | — | — | — | | | |
| 6 | [MiMo V2.6 Pro](https://huggingface.co/XiaomiMiMo/MiMo-V2.6-Pro-RL) | 1,0T (42B ativos) | 600 | 1000 | 📝→📝 | 30 | — | — | — | — | — | | | |

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
- **AA (Artificial Analysis):** índice v5.0 lido em 2026-10-28 em artificialanalysis.ai/models/open-source (11 modelos abertos listados). O Qwen2.5 72B e o Llama 3.3 70B não aparecem pontuados nessa versão. O valor do Sonnet 5.5 (54) veio de uma única leitura do ranking. Outras fontes citam escalas diferentes: não compare índices de versões distintas.
- **Modelos novos (Nv 4 e 6):** só as fichas foram abertas. **Não confirmei** se o llama.cpp roda cada arquitetura, nem se existe GGUF; por isso o link vai à ficha original. A contagem de parâmetros do DeepSeek V4.1 Flash difere (552B no AA, 763B na ficha); a RAM usa a do AA.
- **RAM dos MoE gigantes:** ainda que só 12B a 42B calculem por token, o modelo inteiro precisa caber na RAM somada dos workers (de 72 GB a 600 GB em Q4).
