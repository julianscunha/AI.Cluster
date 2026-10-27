---
title: "Início"
aliases: ["Home", "MOC raiz", "Mapa do repositório"]
tags: [moc]
type: sistema/moc
---

# Início

Ponto de entrada da documentação do **AI Cluster**: um runtime de IA distribuído, Linux-first, em que máquinas heterogêneas viram um único cluster de computação para inferência. As regras de formato estão em [[convencoes]].

## Pastas

| Pasta | Responde | Entrada |
|---|---|---|
| `docs/guia/` | Como instalar, configurar, usar e operar. | [[guia]] |
| `docs/system/` | Como o sistema funciona e como está estruturado: arquitetura, subsistemas, protocolo, modelo de dados e decisões. | [[system]] |
| `docs/implementation/` | O que ainda falta construir. | [[pendencias]] |

## Por onde começar

1. [[visao-geral]]: escopo, ideia central e cargas de trabalho.
2. [[principios]] e [[requisitos]]: o que guia as decisões e o que o sistema faz.
3. [[arquitetura]]: documento mestre; depois [[subsistemas]] para cada peça.
4. [[decisoes]]: ADRs, por que as escolhas foram feitas.

## Mapa de leitura

- **Quero rodar o sistema:** [[guia]] → instalação → configuração → operação.
- **Quero entender o sistema:** [[system]] → [[arquitetura]] → [[subsistemas]] → [[protocolo]] → [[modelo-de-dados]].
- **Quero saber o que falta:** [[pendencias]].
- **Quero mudar o código:** `CONTRIBUTING.md` e `AGENTS.md` na raiz do repositório.

> [!note] Fora do vault
> `README.md`, `AGENTS.md` e os READMEs de `code/` ficam fora de `docs/` e usam links relativos, porque o GitHub não renderiza wikilinks.
