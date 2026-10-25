---
title: "Hot-plug e recuperação"
aliases: ["Hot-Plug and Recovery", "docs/20", "20 — Hot-Plug and Recovery"]
tags: [sistema/subsistema/execucao, subsistema/hotplug-e-recuperacao]
type: sistema/subsistema
status: decidido
implementacao: parcial
fases: ["[[fase-01-nucleo-do-cluster]]", "[[fase-03-execucao-distribuida]]", "[[fase-06-estado-e-recuperacao]]"]
---

# Hot-plug e recuperação

Voltar para [[execucao]].

## Objetivo

Permitir que nós entrem e saiam do cluster sem operação manual, sem perder trabalho e sem corromper o grafo em execução. A regra de base do [[adr-006-hotplug]] é que o cluster se reorganiza sozinho quando a capacidade muda.

## Entrada

```text
DISCOVERED
 -> TRUST
 -> PROVISION
 -> BENCHMARK
 -> warm model/state
 -> READY
```

Um nó só recebe trabalho depois de confiado e medido. Estado de modelo e de dados é carregado antes de ele ficar pronto.

## Remoção planejada

```text
ACTIVE
 -> DRAINING
 -> no new work
 -> finish/checkpoint active work
 -> release model/state
 -> OFFLINE
```

Durante a drenagem o nó não recebe trabalho novo. O trabalho ativo termina ou gera checkpoint, o modelo e o estado são liberados, e só então o nó sai.

## Perda inesperada

```text
heartbeat lost
 -> mark FAILED
 -> identify impacted Work Units
 -> recover state
 -> choose replacements
 -> new plan
 -> resume
```

O controller percebe a perda pelo silêncio do heartbeat, identifica as unidades afetadas, recupera o estado que elas precisam e as reatribui a outros nós, dentro de um novo plano.

## Atualização de capacidade

Depois de entrada ou saída:

1. atualizar o estado do cluster;
2. atualizar o grafo;
3. atualizar a capacidade de desempenho;
4. gerar um plano candidato;
5. comparar a melhoria esperada;
6. replanejar se valer a pena.

## Não-objetivo

Não é requisito inicial migrar memória viva arbitrária, byte a byte, entre hosts. A recuperação privilegia objetos de estado, checkpoints e recomputação seletiva ([[state-manager]]).

> [!info] O que já existe
> **Entrada**
> - O registro (`Registry::register`, `code/crates/controller/src/registry.rs`) põe o nó em `READY` com a capability reportada. Token de join obrigatório em toda RPC ([[f07-token-de-join]]).
> - Aprovação manual: nó novo fica aguardando (`awaiting_approval`) e não recebe trabalho até ser aprovado ([[f07-aprovacao-de-no]]).
> - Não há estados `DISCOVERED`, `PROVISIONING`, `VALIDATING` nem `BENCHMARKING` no controller. Confiança é um flag. Não há carga de modelo ou de estado antes de `READY`.
>
> **Saída planejada**
> - `leave_node` (`code/crates/controller/src/service.rs`): `begin_drain` (vira `DRAINING` e sai dos candidatos), `requeue_for_node`, remoção do registro. O worker para o heartbeat antes de chamar a RPC, para que um beat atrasado não desfaça a saída. Ver [[f01-entrada-e-saida-de-nos]] e [[f03-tolerancia-a-falhas]].
>
> **Perda de nó**
> - O sweep do controller (`code/crates/controller/src/main.rs`) roda a cada 5 s. `mark_stale_offline` marca como `OFFLINE` o nó sem heartbeat por `AI_CLUSTER_HEARTBEAT_GRACE_MS` (15 s por padrão). O heartbeat seguinte devolve o nó a `READY`.
> - `requeue_for_node` devolve as unidades em voo à fila enquanto o orçamento de tentativas dura (`MAX_ATTEMPTS = 3`). Esgotado, a unidade falha com seus dependentes.
> - Evidência: `bash code/tests/phase-2-failure.sh`.
>
> **Reinício do controller**
> - Donos de unidades em voo que não voltam após o grace do start são reenfileirados (`unknown_owners`). O estado vem do snapshot, e uma cópia fica em nós com `AI_CLUSTER_STATE_REPLICA`. Ver [[f06-recuperacao]].
> - Evidência: `bash code/tests/phase-i4-recovery.sh` e `bash code/tests/phase-i4-replica.sh`.
>
> **Revogação**
> - `revoke_node` (S5) tira a confiança e devolve as unidades em voo à fila. Ver [[f07-operador-e-credencial-por-no]].

> [!todo] Não implementado
> - Drenagem que espera o trabalho ativo terminar. Hoje as unidades em voo voltam à fila na hora da saída.
> - Checkpoint de trabalho ativo e liberação de modelo ou estado na saída.
> - Estado `FAILED` para nó que parou de responder. O controller usa `OFFLINE`. `FAILED` e `QUARANTINED` existem no enum, mas nenhum caminho os produz.
> - Estados de entrada `DISCOVERED`, `PROVISIONING`, `VALIDATING`, `BENCHMARKING` e `ACTIVE`/`DEGRADED`.
> - Aquecimento de modelo e de estado antes de `READY` ([[model-manager]]).
> - Replanejamento com comparação de melhoria esperada. Hoje a reavaliação é implícita, a cada poll de trabalho.
> - Capacidade agregada atualizada após entrada e saída ([[modelo-de-desempenho]]).
> - Sinal de falha vindo do worker. Hoje a perda só é notada pelo silêncio do heartbeat.
> - Recuperação incremental. Unidade reoferecida recomeça do zero.
> - Reatribuição que evite o nó que falhou.

> [!warning] Divergência do MVP
> A arquitetura-alvo separa a saída planejada (drenar, checkpoint, liberar) da perda (`FAILED`, replanejar). No MVP, as duas usam o mesmo caminho de devolver à fila, e a perda passa por `OFFLINE`. Para o usuário, o efeito é o mesmo para unidades puras, porque a unidade recomeça do zero. A diferença aparece em estado de longa duração, que o MVP ainda não tem. Migração prevista: o [[state-manager]] passa a produzir checkpoints de estado por chave, e a saída planejada passa a esperar a drenagem.
