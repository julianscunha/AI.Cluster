---
title: "Plataforma"
aliases: ["Plataforma", "02-subsistemas/plataforma"]
tags: [sistema/moc, sistema/subsistema/plataforma]
type: sistema/moc
status: vivo
implementacao: parcial
---

# Plataforma

Subsistema que cobre o sistema operacional dos nós, a forma como uma máquina vira nó e como o
cluster garante identidade, autorização e confiança. Voltar para [[subsistemas]].

## Notas

- [[ai-cluster-os]]: sistema operacional mínimo e imutável de cada nó, com atualização A/B. Não iniciado.
- [[provisionamento]]: como uma máquina com Linux qualquer vira nó com o AI Cluster OS, do descobrimento
  à atestação. Não iniciado; a entrada no cluster pelo worker já existe.
- [[seguranca]]: identidade, papéis de credencial, registro e aprovação, transporte, quarentena,
  revogação, atestação e auditoria. Parcial: segredos compartilhados e credencial por nó no MVP.

## Ordem de leitura

[[ai-cluster-os]] → [[provisionamento]] → [[seguranca]]. A segurança fecha o ciclo: a imagem define o
que o nó é, o provisionamento prova que ele chegou íntegro, e a segurança decide o que ele pode fazer.

> [!info] O que já existe
> A entrada e a autenticação dos nós vivem em [[f07-token-de-join]], [[f07-aprovacao-de-no]] e
> [[f07-operador-e-credencial-por-no]]. A imagem de SO e o provisionamento ainda não existem.
