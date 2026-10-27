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
