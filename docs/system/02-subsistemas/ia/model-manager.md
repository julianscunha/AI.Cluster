---
title: "Model Manager / Registry"
aliases: ["Gerenciador de modelos", "Registry de modelos", "Model Manager"]
tags: [sistema/subsistema/ia]
type: sistema/subsistema
---

# Model Manager / Registry

O Model Manager diz quais pesos existem, em qual versão e com quais checksums. Ele sabe **o que existe e onde está**; o [[execution-planner]] decide **onde deveria estar**. Voltar para [[ia]].

Há duas camadas, e o [[adr-022-fluxo-de-modelos-e-motor-hibrido]] explica a decisão:

1. **Catálogo e orquestração (llama.cpp, GGUF).** O controller varre uma pasta de modelos, confere a RAM dos workers e manda o gateway carregar o modelo. É o caminho do produto.
2. **Diretórios de pesos do motor próprio (safetensors).** GPT-2 e SmolLM2, com manifesto. É a base de referência de paridade e o caminho antigo das fatias residentes.

## Catálogo, OK/NOK e `model start` (llama.cpp)

O controller lê `AI_CLUSTER_MODELS_DIR` (um nível: `*.gguf` ou pastas com `config.json` e `*.safetensors`) pelo **cabeçalho**, sem carregar pesos (`crates/catalog`):

- **GGUF:** arquitetura, parâmetros, precisão (`Q4_K_M`…), contexto, camadas e dimensões. Lê em sequência, com tetos de leitura, soma as partes de um GGUF dividido e ignora atalhos (symlinks).
- **safetensors:** parâmetros e precisão pelo cabeçalho; `supported` só para `gpt2` e `llama` (motor próprio).
- **Campo `supported` e `reason`:** o modelo roda ou o motivo de não rodar. `model start` exige GGUF.

Comandos (veja [[cli]] no guia): `model list`, `model check <id> [--ctx N]`, `model start <id> [--ctx N]`, `model stop`, `model status`.

- **OK/NOK por RAM** (`catalog::check`): `precisa = pesos + cache KV (2·2·camadas·contexto·largura·kv/cabeças bytes, f16) + 10% de margem`; `tem` = soma da RAM dos workers com RPC, aprovados e vivos (no máximo 80% de cada um, menos 128 MiB). O NOK diz quanto falta e quantos workers do tamanho médio acrescentar.
- **Plano de workers** (`controller/src/models.rs`): `select` ordena os workers pela velocidade da CPU e usa os mais rápidos **só até a RAM somada comportar o modelo** (cada worker é um salto de rede por token; um lento só entra quando falta memória). A divisão de camadas (`--tensor-split`) é proporcional à velocidade, limitada pelo que cada worker comporta.
- **Geração:** sobe a cada `start`, `stop` ou mudança estável do plano. Uma lista nova de workers só vira plano depois de `AI_CLUSTER_PLAN_STABLE_MS` (30 s) igual, para um worker que oscila não recarregar o modelo.
- **Sem RAM:** se os workers de agora não comportam o pedido, o modelo para com o motivo (`FALHOU`) e volta sozinho quando a RAM volta.
- **Chaves de API:** o primeiro `start` cria o tenant `default` e mostra a chave **uma vez**. O controller guarda `sha256(chave)` em `AI_CLUSTER_MODELS_STATE` (ou `$AI_CLUSTER_DATA_DIR/models.state`, escrita atômica, só para o dono); o gateway recebe só os hashes.
- **Gateway:** a cada apresentação recebe o `DesiredModel` e as chaves em hash; aplica com o `Manager` (túneis mTLS locais e `llama-server`), reporta o estado (`CARREGANDO`, `PRONTO`, `FALHOU`) e o benchmark (tokens/s de um prompt fixo de 64 tokens).

## Diretório de modelo (motor próprio, safetensors)

Um modelo é um diretório com:

- `model.safetensors`: os pesos. O nome do arquivo é fixo.
- Arquivos do tokenizador (`vocab.json`, `merges.txt`, `config.json`, e `tokenizer.json` quando houver).
- `manifest.json`: nome, versão e SHA-256 de cada arquivo. Opcional, mas obrigatório se `AI_CLUSTER_REQUIRE_MANIFEST=1`.

O gateway lê diretórios por variável (`AI_CLUSTER_GPT2_DIR`, `AI_CLUSTER_SMOLLM2_DIR`, `AI_CLUSTER_SMOLLM2_INSTRUCT_DIR`). Os workers lêem `AI_CLUSTER_MODEL_DIR/<subdiretório>/model.safetensors`. O subdiretório aceita só componentes simples, é canonicalizado e precisa continuar dentro da raiz.

## Manifesto

Gerar e conferir pela CLI, sem controller:

```bash
ai-cluster model manifest <diretório> --name <nome> --version <versão>
ai-cluster model verify <diretório>
```

- O manifesto lista os arquivos em ordem de nome, com tamanho e SHA-256 de cada um. Os mesmos pesos geram o mesmo manifesto.
- Nome e versão aceitam letras, dígitos, `.`, `_` e `-`, e são seguros em cabeçalho HTTP.
- O manifesto tem no máximo 1024 arquivos e 1 MiB, e precisa listar `model.safetensors`.

Onde a versão aparece:

- **Gateway:** na subida, cada diretório com manifesto é conferido. Se algum arquivo não confere, o gateway não sobe e a mensagem nomeia o arquivo. A versão sai em `GET /v1/models` (`version`) e no cabeçalho `x-model-version` das respostas de chat.
- **Worker:** antes de abrir o canal de fatias, confere o manifesto de cada subdiretório de `AI_CLUSTER_MODEL_DIR`. Divergência encerra o worker. Com `AI_CLUSTER_REQUIRE_MANIFEST=1`, subdiretório sem manifesto também é recusado.

## Versão e fatias

A chave de uma faixa de peso no worker é (subdiretório, tensor, linhas). Ela não tem versão. Por isso, trocar os pesos de um modelo com o cluster rodando exige reiniciar os workers, senão faixas antigas continuam em memória.

## Limites conhecidos

- **Conferência só na subida.** Um arquivo trocado depois do início não é detectado, e o worker lê as faixas do disco sob demanda.
- **Links simbólicos são seguidos.** Quem controla o diretório controla o alvo.
- **Manifesto não é assinado.** Quem escreve no diretório escreve no manifesto. Ele detecta corrupção e divergência, não adulteração por quem tem acesso ao disco.
- **Sem registry central de versões.** O gateway e os workers não comparam versões entre si. O controller não confere a `rpc_version` do `rpc-server` com a do `llama-server`.
- **Motor próprio:** uma só variante por modelo (float32 ou BF16, sem quantização). A quantização vem do GGUF, pelo llama.cpp.
- **Catálogo:** varre um nível da pasta e recalcula a cada chamada; não há cache nem verificação de checksum dos GGUF.
- **Pesos no gateway:** o GGUF precisa estar na mesma pasta (`AI_CLUSTER_MODELS_DIR`) no gateway; o llama.cpp envia os tensores aos workers pelo RPC, então os workers não guardam o modelo.

> Planejado: registry com as entidades Model, Version, Variant, Manifest, Artifact e Shard.
> Planejado: variantes de precisão e quantização (Q8, Q4), tratadas como atributo de uma variante, não como outro modelo.
> Planejado: perfil de compatibilidade (arquitetura, memória mínima, precisão e backends) consultado pelo planejador antes de atribuir uma faixa.
> Planejado: execução que referencia versão e variante exatas, nunca mutáveis no meio de uma requisição.
> Planejado: verificação de checksum a cada faixa carregada, e cache em três camadas (memória, SSD, repositório) com prefetch.
> Planejado: manifesto assinado.
