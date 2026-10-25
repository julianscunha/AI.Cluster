---
title: "Início"
aliases: ["Home", "MOC raiz", "Mapa do repositório", "DIRECTORY-MAP"]
tags: [sistema/moc, moc]
type: sistema/moc
status: vivo
implementacao: implementado
---

# Início

Ponto de entrada da documentação do **AI Cluster**: um runtime de IA distribuído, Linux-first, em que máquinas heterogêneas viram um único cluster de computação para inferência. O repositório inteiro é um *vault* do Obsidian; as regras de formato estão em [[convencoes]].

## Duas pastas

| Pasta | Responde | Entrada |
|---|---|---|
| `docs/system/` | O que o sistema é e como funciona — a arquitetura-alvo completa, inclusive o que ainda não existe. | [[system]] |
| `docs/implementation/` | Como e quando construir — 20 fases por dependência (`F00`…`F19`), slices, status, testes e evidências. | [[implementation]] |

## Por onde começar

1. [[visao-geral]] — escopo, ideia central e cargas de trabalho.
2. [[principios]] e [[requisitos]] — o que guia as decisões e o que o sistema precisa fazer.
3. [[arquitetura]] — documento mestre; depois [[subsistemas]] para cada peça.
4. [[decisoes]] — ADRs: por que as escolhas foram feitas.
5. [[status]] — o que já está implementado, parcial ou por fazer, e [[implementation]] para a ordem das fases.

## Mapa de leitura

- **Quero entender o sistema:** [[system]] → [[arquitetura]] → [[subsistemas]] → [[protocolo]] → [[modelo-de-dados]].
- **Quero saber o que existe e o que falta:** [[status]] → [[implementation]] → a nota da fase.
- **Quero rodar ou testar:** [[estrategia-de-testes]] e [[evidencias-e2e]].
- **Quero mudar o código:** `AGENTS.md` na raiz do repositório e o slice da fase correspondente.

> [!note] Fora do vault
> `README.md`, `AGENTS.md` e os READMEs de `code/` ficam fora de `docs/` e usam links relativos, porque o GitHub não renderiza wikilinks.
