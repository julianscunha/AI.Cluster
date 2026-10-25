---
title: "Model Manager / Registry"
aliases: ["Gerenciador de modelos", "Registry de modelos", "docs/14", "14 — Model Manager / Registry"]
tags: [sistema/subsistema/ia]
type: sistema/subsistema
status: proposto
implementacao: nao-iniciado
fases: ["[[fase-12-model-runtime]]"]
---

# Model Manager / Registry

O Model Manager gerencia modelos, versões, variantes de precisão e shards, independentemente do placement. Ele sabe **o que existe e onde está**; o [[execution-planner]] decide **onde deveria estar**. Voltar para [[ia]].

## Entidades do registry

| Entidade | Conteúdo |
|---|---|
| Model | Identidade conceitual (ex.: `llama-x-3.2`) |
| Version | Revisão imutável dos pesos e da configuração |
| Variant | Versão em uma precisão ou quantização (FP16, FP8, INT8, Q8, Q6, Q5, Q4) |
| Manifest | Lista de artefatos, shards, tamanhos e checksums de uma variante |
| Artifact | Arquivo físico (safetensors, GGUF ou equivalente) |
| Shard | Fatia de um artefato atribuível a um nó |
| Compatibility profile | Arquitetura, memória mínima, precisão e backends suportados |

A quantização é atributo de uma variante, não outro modelo conceitual.

## Referência de execução imutável

Uma execução referencia uma variante e uma versão exatas e não as altera no meio da request. Trocar de versão é nova execução, nunca mutação.

## Cache de modelo

| Camada | Local | Uso |
|---|---|---|
| HOT | RAM/VRAM | Shards em uso |
| WARM | NVMe/SSD | Shards recentes; política de [[storage-intelligence]] |
| COLD | Repositório | Origem |

Prefetch do próximo shard quando houver previsibilidade suficiente.

## Compatibilidade

Cada shard ou variante declara arquitetura, memória mínima, precisão e backends suportados. O planner só atribui shards a nós compatíveis com esse perfil.

> [!info] O que já existe (MVP)
> Não há registry, manifest, versão nem checksum. O que existe é o carregamento direto de arquivos por caminho:
> - Gateway: `AI_CLUSTER_GPT2_DIR`, `AI_CLUSTER_SMOLLM2_DIR` e `AI_CLUSTER_SMOLLM2_INSTRUCT_DIR` apontam para os arquivos de cada modelo. O gateway carrega na subida e falha fechado se o diretório estiver incompleto.
> - Workers: lêem as fatias de `AI_CLUSTER_MODEL_DIR/<subdir>/model.safetensors`, na primeira vez que a fatia é pedida, e mantêm em `ShardStore` com despejo LRU. O subdiretório é validado (componentes simples, canonicalizado, sob a raiz); o nome do arquivo é fixo.
> - Modelos em `code/models/` ficam fora do repositório (ignorados pelo git).
>
> Os pesos não são versionados nem verificados: quem garante que o worker tem o mesmo arquivo que o gateway é o operador. A resposta da API não traz `model_version`. Slices: [[f08-engine-local]], [[f08-canal-residente-de-fatias]].

> [!warning] Desvio do alvo e migração
> O cluster não tem um Model Manager. A referência de execução é um caminho, não uma versão; o mesmo nome de subdiretório em dois workers pode carregar pesos diferentes sem que nada detecte. Migração: registry com manifest e checksum em [[fase-12-model-runtime]].

> [!todo] Falta
> - Registry com Model, Version, Variant, Manifest, Artifact e Shard.
> - Verificação de checksum na carga de cada shard e na distribuição.
> - Perfil de compatibilidade consultado pelo planner.
> - Referência de execução imutável (variante e versão) e retorno de `model_version` na API.
> - Cache HOT/WARM/COLD e prefetch.
> - Variantes quantizadas (Q4/Q8) no [[inference-engine]].
