---
title: "CLI e TUI"
aliases: ["docs/22", "22 — CLI / TUI"]
tags: [sistema/subsistema/operacao]
type: sistema/subsistema
status: proposto
implementacao: parcial
fases: ["[[fase-05-observabilidade]]", "[[fase-07-seguranca-base]]"]
---

# CLI e TUI

A operação do cluster é feita pelo shell. A CLI `ai-cluster` é o único cliente administrativo: cada
comando é uma chamada ao controller, sem estado próprio. A TUI, um painel que se atualiza, é a mesma
leitura em tela cheia. Nenhum dos dois depende de interface gráfica.

## Regras

- Operação por shell. Toda saída que um script lê é `chave=valor` ou tabela de colunas fixas.
- A CLI usa o token de operador ([[seguranca]]). Nenhum comando de leitura funciona sem ele.
- O endereço do controller vem de `AI_CLUSTER_CONTROLLER`.
- Estado sempre com texto. Cor, quando existir, acompanha a palavra, e não a substitui; assim o
  painel funciona em terminal sem cor, em `grep` e em operação remota.

## Comandos

Alvo, como especificado:

```bash
ai-cluster status
ai-cluster dashboard
ai-cluster nodes
ai-cluster node show <id>
ai-cluster node drain <id>
ai-cluster node reprovision <id>
ai-cluster node revoke <id>
ai-cluster model list
ai-cluster model show <id>
ai-cluster request list
ai-cluster request show <id>
ai-cluster request trace <id>
ai-cluster metrics
ai-cluster events
ai-cluster topology
ai-cluster graph query <query>
ai-cluster diagnose
ai-cluster benchmark
ai-cluster rebalance
```

## Painel

Alvo: tempo real com contagem e estado dos nós, saúde, capacidade, CPU, RAM, GPU e NPU, temperatura,
saúde de armazenamento, rede, requisições, TTFT, ITL, tokens por segundo, tokens evitados, cache KV,
esperado vs real e plano atual.

Semântica de estado (alvo):

| Cor | Estado |
|---|---|
| Verde | normal |
| Amarelo | degradado |
| Laranja | alerta ou drenando |
| Vermelho | falha ou quarentena |
| Azul | provisionamento ou transição |
| Cinza | offline |

> [!info] O que já existe
> - Comandos implementados em `code/crates/cli/src/main.rs`: `status`, `nodes`, `node show <id>`,
>   `metrics`, `events [--limit N]`, `request trace <id>`, `benchmark`, `graph [--size]` (dispatch do
>   grafo em diamante), `reserve <nó> <requisição> [--ttl-ms]`, `reservations`, `approve <nó>`,
>   `revoke <nó>`, `release <nó>`, `dashboard [--watch N]`.
> - `node show`: [[f02-capability-corrente]]. `benchmark`: [[f02-benchmark-de-capability]]. `graph`:
>   [[f03-grafo-de-dependencias]]. `reserve`, `reservations` e `release`: [[f04-reservas]].
> - `approve` e `revoke`: [[f07-aprovacao-de-no]] e [[f07-operador-e-credencial-por-no]].
> - `metrics`: [[f05-metricas]]. `events`: [[f05-eventos]]. `request trace`: [[f05-traces]].
> - `dashboard`: [[f05-dashboard]]. Lê `ListNodes`, `GetMetrics` e `ListReservations`; sem RPC novo. A
>   atualização repinta a tela inteira a cada N segundos, sem streaming. Não há navegação nem seleção.
> - Estado do painel só em texto, sem cor, por escolha registrada em [[f05-dashboard]].

> [!todo] Comandos e painel que faltam:
> - `node drain`, `node reprovision` e `node history`. O drain existe só do lado do worker
>   (`LeaveNode`), não há comando do operador.
> - `request list` e `request show`. Só o `trace` existe.
> - `model list` e `model show`, que dependem do model manager ([[model-manager]]).
> - `topology`, `diagnose`, `rebalance` e `graph query`.
> - Painel: TTFT, ITL, tokens por segundo, tokens evitados, cache KV, esperado vs real no painel,
>   plano atual e a faixa de saúde de armazenamento e rede.
> - Cor no painel e TUI com navegação. Hoje é texto repintado.

> [!warning] Divergências de forma: `revoke` está no topo (`ai-cluster revoke <nó>`), não sob `node`.
> todos os comandos (`status`, `nodes`, `metrics`, `events`, `dashboard`) usam a conexão de operador,
> então exigem o token de operador mesmo para leitura. A CLI não tem papel de leitura separado.
