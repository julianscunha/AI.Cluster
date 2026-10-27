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

## Identidade visual

Todo componente fala com a mesma cara, definida em `crates/common/src/brand.rs` (Rust) e em `os/image/overlay/usr/lib/aic/ui.sh` (scripts do nó). Mexa nos dois juntos.

- **Logotipo:** uma malha de sete nós em cinco linhas, em ciano. Aparece no banner de partida do controller, do gateway e do worker, no painel (`ai-cluster dashboard`), no console do monitor e na tela de carregamento do nó.
- **Paleta por papel, não por componente:** título forte (branco), rótulo e logotipo em ciano, **verde** para pronto, **amarelo** para atenção (pendente, drenando), **vermelho** para falha ou sem sinal, cinza para apoio. `READY` e `VIVO` são verdes; `OFFLINE`, `QUARANTINED` e `SEM SINAL` são vermelhos; o resto é amarelo.
- **Só onde a tela aceita:** cor e glifos de caixa saem num terminal de verdade. Saída redirecionada (log de contêiner, arquivo, scripts, testes) fica em texto puro, e o painel volta ao formato `chave=valor` que os scripts leem. `NO_COLOR` e `AI_CLUSTER_COLOR=never` desligam a cor, `AI_CLUSTER_COLOR=always` força, `AI_CLUSTER_ASCII=1` troca os glifos por ASCII.
- **Linha de partida:** num log, o banner é uma linha só (`Real.AI.Cluster <componente> v<versão> - <detalhe>`), seguida pelas linhas de sempre.

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
