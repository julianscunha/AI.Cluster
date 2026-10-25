---
title: "Execution Graph"
aliases: ["Grafo de execução", "docs/09", "09 — Execution Graph"]
tags: [sistema/subsistema/execucao, subsistema/execution-graph]
type: sistema/subsistema
status: decidido
implementacao: parcial
fases: ["[[fase-03-execucao-distribuida]]"]
---

# Execution Graph

Voltar para [[execucao]].

## Objetivo

Representar uma requisição como um grafo de operações com dependências explícitas. Operações independentes podem rodar em paralelo; uma operação que depende de outra espera exatamente pelo dado de que precisa, e nada mais.

## Model Graph e Execution Graph

- **Model Graph** descreve a arquitetura do modelo: operadores, formas e ligações. É estático por modelo e variante ([[modelo-de-grafo]], [[adr-004-grafo-como-projecao]]).
- **Execution Graph** descreve uma execução concreta, considerando a requisição, a precisão, os caches, o hardware disponível e a política. Um mesmo Model Graph gera Execution Graphs diferentes conforme a requisição e o cluster.

## Elementos

- **Nó do grafo**: uma [[work-unit]], com identificador próprio e vinculada a uma requisição.
- **Dependência de controle** (`depends_on`): a unidade só pode ser despachada depois que todas as unidades listadas completaram.
- **Ligação de dado** (entrada vinculada): uma entrada da unidade é a saída de outra unidade. A saída só existe depois que a origem completa.
- **Saída intermediária**: resultado retido enquanto alguma unidade ainda não concluída precisa dele.

## Invariantes

1. Nenhuma unidade é entregue antes de suas dependências completarem e de suas entradas vinculadas estarem disponíveis.
2. Um resultado só é aceito do nó para o qual a unidade foi despachada. Resultado atrasado de nó que perdeu a posse é recusado.
3. Falha de uma unidade falha também seus dependentes, transitivamente. Nenhum dependente fica pendurado para sempre.
4. O significado lógico do grafo não depende do placement: trocar o nó que roda uma unidade não muda o resultado.
5. Uma unidade devolvida à fila (nó saiu ou falhou) mantém dependências e ligações.

## Paralelismo e barreiras

Operações independentes ficam prontas ao mesmo tempo e podem ir a nós distintos:

```text
W1 ----+
       |
W2 ----+--> SYNC --> W4
       |
W3 ----+
```

Barreiras só locais, onde uma unidade depende de um conjunto específico. Barreiras globais são evitadas, porque deixam o grafo inteiro esperar pelo ramo mais lento.

## Versionamento

Cada Execution Graph está associado a uma versão de plano ([[execution-planner]]). Mudança de placement gera nova versão de plano e não altera o grafo lógico.

## Recuperação

Se um nó falha, as unidades afetadas voltam para a fila dentro de um orçamento de tentativas. Esgotado o orçamento, a unidade e seus dependentes falham. Detalhes em [[hotplug-e-recuperacao]].

> [!info] O que já existe
> - Dependências e ligações: `WorkUnit.depends_on`, `matrix_a_from` e `matrix_b_from` em `code/proto/cluster.proto`. `ready` e `take_locked` em `code/crates/controller/src/work.rs` aplicam os invariantes 1 e 5. Ver [[f03-grafo-de-dependencias]].
> - Invariante 2: `WorkStore::complete` recusa resultado vindo de nó diferente do despachado.
> - Invariante 3: `fail_cascade` e `MAX_ATTEMPTS = 3` em `work.rs`. Ver [[f03-tolerancia-a-falhas]].
> - Retenção de saídas: `drop_unreferenced_outputs` descarta a saída que nenhuma unidade na fila ou em voo lê. O estado é gravado no snapshot de [[state-manager]]. Ver [[f06-persistencia-e-checkpoint]].
> - Montagem de grafo: o RPC `EnqueueGraph` enfileira o diamante A→(B,C)→D; `ai-cluster graph` o envia e drena. Evidência: `bash code/tests/phase-2-graph.sh`, com checksums diferentes entre as unidades, prova de que o dado fluiu.
> - Inferência por fatias (`ShardMatmul`, canal residente) não passa por esta fila. Ver [[f08-canal-residente-de-fatias]].

> [!todo] Não implementado
> - Grafo derivado do Model Graph e do perfil da requisição (precisão, caches, política). O cliente envia unidades prontas.
> - Barreiras locais explícitas. Só há dependência ponto a ponto.
> - Versão de plano por grafo.
> - Caminho direto worker → worker. Hoje o dado passa pelo controller duas vezes (ida e volta).
> - Consulta de prontidão sem varredura linear da fila. Hoje `claim` percorre a fila inteira (O(n)).

> [!warning] Divergência do MVP
> Na arquitetura-alvo, quem monta o grafo é o planejador ([[execution-planner]]) a partir do Model Graph. No MVP, a fila de Work Units é a única via de despacho do controller, mas a inferência por fatias escolhe os nós por variável de ambiente (`AI_CLUSTER_<MODELO>_SHARDS`, lida no gateway) e não passa pela fila nem pelo placement. Migração prevista: o [[request-manager]] e o planejador passam a montar o grafo; até lá, a inferência fica fora deste subsistema.
