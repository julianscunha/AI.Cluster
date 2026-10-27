---
title: "Model Manager / Registry"
aliases: ["Gerenciador de modelos", "Registry de modelos", "Model Manager"]
tags: [sistema/subsistema/ia]
type: sistema/subsistema
---

# Model Manager / Registry

O Model Manager diz quais pesos existem, em qual versão e com quais checksums. Ele sabe **o que existe e onde está**; o [[execution-planner]] decide **onde deveria estar**. Voltar para [[ia]].

Hoje não há um registry central. Cada modelo é um diretório, e o diretório carrega o próprio manifesto.

## Diretório de modelo

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
- **Sem registry central.** Gateway e workers não comparam versões entre si. O cluster não sabe se todos servem a mesma versão.
- **Uma só variante por modelo.** Os pesos são float32 ou BF16, sem quantização.

> Planejado: registry com as entidades Model, Version, Variant, Manifest, Artifact e Shard.
> Planejado: variantes de precisão e quantização (Q8, Q4), tratadas como atributo de uma variante, não como outro modelo.
> Planejado: perfil de compatibilidade (arquitetura, memória mínima, precisão e backends) consultado pelo planejador antes de atribuir uma faixa.
> Planejado: execução que referencia versão e variante exatas, nunca mutáveis no meio de uma requisição.
> Planejado: verificação de checksum a cada faixa carregada, e cache em três camadas (memória, SSD, repositório) com prefetch.
> Planejado: manifesto assinado.
