---
title: "Operação"
aliases: ["Operação", "02-subsistemas/operacao"]
tags: [sistema/moc, sistema/subsistema/operacao]
type: sistema/moc
status: vivo
implementacao: parcial
---

# Operação

Subsistema que mostra o cluster a quem opera: métricas, eventos, traces, painel e a CLI que
dá acesso a tudo isso. Voltar para [[subsistemas]].

## Notas

- [[observabilidade]]: os três tipos de telemetria (métricas, eventos e traces), previsto vs real e o
  que cada um responde. Parcial: cluster e nó têm métricas, eventos e traces; inferência e
  diagnóstico ainda não.
- [[cli-tui]]: a CLI `ai-cluster` e o painel de tela única. Parcial: comandos de leitura, revogação e
  reserva existem; drain, histórico de nó, modelos e diagnóstico não.

## Ordem de leitura

[[observabilidade]] → [[cli-tui]]. A CLI é a interface de todo dado da observabilidade; ler primeiro o
que é medido torna a lista de comandos clara.

> [!info] O que já existe
> Os slices de observabilidade são `f05-eventos`, `f05-metricas`, `f05-traces`, `f05-dashboard` e
> `f05-previsto-vs-real`, em [[fase-05-observabilidade]]. A CLI cresce junto com as fases que
> entregam cada comando.
