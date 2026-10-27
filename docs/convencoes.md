---
title: "Convenções da documentação"
aliases: ["Convenções", "Guia de estilo"]
tags: [moc, convencao]
type: sistema/moc
---

# Convenções da documentação

`docs/` é um *vault* do Obsidian e também se lê no GitHub. Este guia vale para tudo em `docs/`. Voltar para [[inicio]].

## Pastas

| Pasta | Conteúdo |
|---|---|
| `docs/guia/` | Instalação, configuração, CLI, API, operação e testes. |
| `docs/system/` | Como o sistema funciona: visão, princípios, requisitos, arquitetura, subsistemas, protocolo, modelo de dados, decisões (ADRs). |
| `docs/implementation/` | Só o que falta fazer. Quando um item termina, ele sai daqui e o comportamento passa para `guia/` ou `system/`. |

## Regra de conteúdo

A documentação descreve **como o sistema é e como usá-lo**, não como foi construído: sem fases, histórico, evidências de laboratório nem nomes ou endereços de máquinas de teste. Capacidade que ainda não existe aparece uma vez, como `> Planejado: …`.

## Nomes de comandos

Dois prefixos, cada um com um papel:

- **`ai-cluster*`** é o software do cluster, o que o operador instala e roda: a CLI `ai-cluster` e os binários `ai-cluster-controller`, `ai-cluster-worker`, `ai-cluster-gateway` e `ai-cluster-benchmark`.
- **`aic-*`** são as ferramentas do SO do nó, que só existem dentro da imagem. A porta de entrada é o comando único `aic <subcomando>`; as ferramentas internas (`aic-console`, `aic-updater`, `aic-update-request`, `aic-splash-run`) são chamadas pelo sistema, não à mão.

Todo comando aceita `--help`.

## Arquivos e links

- Nomes em kebab-case, ASCII, pt-br sem acento. O título com acento vai no frontmatter.
- **Nomes únicos no vault**: `slug` entre colchetes duplos precisa ser inequívoco.
- ADR: `adr-NNN-slug.md`, com `Status` e data no corpo.
- Dentro de `docs/` só wikilinks (`[[slug]]`, `[[slug|texto]]`, `[[slug#Seção]]`); nenhum link relativo para `.md`.
- Código e scripts aparecem em `inline code`, nunca como wikilink.
- Toda pasta tem uma nota-índice (MOC) que lista suas notas.

## Frontmatter

```yaml
---
title: "Scheduler"
aliases: ["Escalonador"]
tags: [sistema/subsistema/execucao]
type: sistema/subsistema   # sistema/{visao|arquitetura|subsistema|protocolo|modelo-de-dados|adr|moc}, guia
---
```

## Callouts

`[!note]` contexto · `[!warning]` limite ou risco · `[!example]` saída de comando · `[!important]` regra.

Idioma: pt-br com acentuação correta; termos técnicos e identificadores ficam como são.
