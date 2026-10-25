---
title: "Convenções da documentação"
aliases: ["Convenções", "Padrão Obsidian", "Guia de estilo"]
tags: [sistema/moc, convencao]
type: sistema/moc
status: vivo
implementacao: implementado
---

# Convenções da documentação

O repositório é um *vault* do Obsidian: a raiz do repositório é a raiz do vault. Este guia vale para tudo em `docs/`. Voltar para [[inicio]].

## Duas pastas

| Pasta | Responde | Conteúdo |
|---|---|---|
| `docs/system/` | *O que é e como funciona* | Arquitetura-alvo completa, **mesmo o que ainda não existe**: visão, princípios, requisitos, subsistemas, protocolo, modelo de dados, decisões (ADRs). |
| `docs/implementation/` | *Como e quando construir* | Planejamento e implementação em **fases** por dependência (`F00`…`F19`), um slice por entrega, status, testes e evidências. |

Na raiz do repositório ficam só `README.md`, `AGENTS.md` e os READMEs de `code/`. Esses usam links relativos (o GitHub não renderiza wikilinks).

## Arquivos e links

- Nomes em kebab-case, ASCII, pt-br sem acento. O título com acento vai no frontmatter.
- **Nomes únicos no vault inteiro**: `[[slug]]` precisa ser inequívoco. Sem prefixo numérico nos docs de sistema; o prefixo numérico fica nas pastas.
- Implementação: nota da fase `fase-NN-nome.md`; slices `fNN-slug.md`. ADR: `adr-NNN-slug.md`.
- Dentro de `docs/` só wikilinks: `[[slug]]`, `[[slug|texto]]`, `[[slug#Seção]]`. Nenhum link relativo para `.md`.
- Código e scripts aparecem em `inline code` (`code/crates/…`), nunca como wikilink.
- Toda pasta tem uma nota-índice (MOC) com o nome da pasta, sem o prefixo numérico, listando suas notas. Todo documento precisa estar ligado a um MOC.

## Frontmatter — sistema

```yaml
---
title: "Scheduler"
aliases: ["Escalonador"]          # nomes antigos viram aliases
tags: [sistema/subsistema/execucao]
type: sistema/subsistema          # sistema/{visao|arquitetura|subsistema|protocolo|modelo-de-dados|adr|moc}
status: decidido                  # decidido|proposto|experimental|aberto|vivo|rascunho
implementacao: parcial            # nao-iniciado|parcial|implementado
fases: ["[[fase-04-escalonamento]]"]
---
```

O corpo descreve a arquitetura-alvo completa. No fim, um `> [!info] O que já existe` com links para os slices `fNN-*`; o que falta usa `> [!todo]`.

## Frontmatter — implementação

```yaml
---
title: "F04 — Placement"
aliases: ["phase-2/placement"]
tags: [implementacao/fase/04, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia         # implementacao/{fase|fatia|transversal|moc}
status: implementado              # planejado|em-andamento|parcial|implementado
superado: false
fase: "[[fase-04-escalonamento]]"
codigos-antigos: ["G1", "G3"]
sistema: ["[[scheduler]]"]
evidencia: ["code/tests/phase-g-placement.sh"]
---
# F04 — Placement

- [ ] Superado
```

> [!important] Regra do "superado"
> O checkbox `- [ ] Superado` fica logo abaixo do `# título` e precisa coincidir com `superado:` no frontmatter. **Superado** significa que o texto não descreve mais o código atual (foi substituído por outro slice ou pela arquitetura-alvo), e não que o trabalho foi concluído. Quando verdadeiro: marcar `- [x] Superado` e abrir com `> [!warning] Superado por [[destino]]`.

Uma nota de fase (`fase-NN-*`) tem as seções: Objetivo · Depende de · Entrega · Checklist · Gate · Slices · Códigos antigos · Specs do sistema. O gate padrão exige código, benchmark, telemetria, teste de falha, doc e ADR.

## Tags, callouts e legenda

- Tags hierárquicas, minúsculas, ASCII: `sistema/…`, `implementacao/fase/NN`, `implementacao/estado/*`, `subsistema/<slug>`, `adr`, `moc`.
- Callouts: `[!note]` contexto · `[!info]` o que existe · `[!todo]` pendente · `[!warning]` teto ou risco · `[!example]` evidência ou saída de comando · `[!important]` regra.
- Legenda de status: `[x]` implementado e validado · `[~]` parcial · `[ ]` não iniciado.
- Idioma: pt-br com acentuação correta; termos técnicos e identificadores de código ficam como são.
