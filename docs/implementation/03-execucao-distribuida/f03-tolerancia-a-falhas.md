---
title: "Fase 2 — Tolerância a falhas"
type: implementation
status: implemented
tags: [implementation, phase-2, fault-tolerance, retry]
---

# Fase 2 — Tolerância a falhas

Reatribuição limitada: uma unidade cujo worker morre é entregue a outro nó, mas só
algumas vezes. Passado o orçamento, a unidade falha — e tudo a jusante dela também —, para
que o grafo sempre chegue a uma resposta em vez de travar.

## O que mudou

- O `WorkStore` conta tentativas de dispatch por work id. O `claim` incrementa o contador;
  a contagem sobrevive a um hand-back porque vive no store, não no registro de dispatch
  que é descartado.
- `requeue_for_node` devolve uma unidade só enquanto o orçamento dura. Passado o
  `MAX_ATTEMPTS` (3: uma primeira tentativa mais duas reatribuições), a unidade falha em
  vez de voltar para a fila.
- `fail_cascade` falha os dependentes de uma unidade junto com ela, transitivamente. Um
  dependente espera pelo *output* da sua origem, e uma unidade falha nunca produz um, então
  sem a cascata o dependente ficaria na fila para sempre e `pending()` nunca chegaria a
  zero.
- `RequeueOutcome` reporta as duas contagens, para que o caminho de saída e o sweep de
  offline possam logar `requeued N, failed M` em vez de sumir com a diferença.
- O heartbeat e o health report do worker rodam em task própria, com intervalo próprio,
  então uma unidade longa não pode mais matá-los de fome. Compartilhar o laço de trabalho
  fazia o controller reapar workers saudáveis no meio do compute. Ver "Liveness" abaixo.
- `WorkReportResponse.failed` expõe a contagem de falhas; o CLI imprime `failed=`.
- O hook de teste `AI_CLUSTER_WORK_DELAY_MS` segura uma unidade reivindicada antes de
  computá-la. Sem ela, sem atraso; existe para que um teste de falha possa matar um worker
  enquanto ele realmente segura trabalho.

## Semântica

As tentativas são contadas quando uma unidade é reivindicada, então o número significa
"vezes que esta unidade foi iniciada", não "vezes que ela voltou". Uma unidade que completa
limpa seu contador; uma unidade que esgota o orçamento vai para o conjunto de falhas e seu
registro de dispatch é descartado, então um resultado atrasado do worker morto é recusado
como sempre.

Unidades falhas não estão na fila nem em voo, então `pending()` as exclui. É esse o ponto:
um cliente que faz poll esperando conclusão termina em qualquer um dos dois desfechos —
drenou ou desistiu — em vez de esperar pelo seu próprio deadline. `failed=0` significa que o
grafo rodou até o fim; uma contagem diferente de zero significa que algum branch não pôde
ser concluído.

A cascata não alcança uma unidade em voo: a prontidão retém um dependente até que sua
origem tenha produzido output, então nada a jusante de uma falha chega a ser despachado.

## Evidência — Docker, dois workers, worker morto no meio da unidade

Reproduza com `bash code/tests/phase-2-failure.sh`. Duas unidades são enfileiradas para que
os dois workers segurem uma cada; cada unidade é segurada por 12s para que o kill caia
enquanto ela está em voo; `node-b` então leva `docker kill`, sem chamada de leave. A grace é
posta em 9s de propósito — menor que o hold — para que um worker saudável fosse reapado se
o heartbeat ainda compartilhasse o laço de trabalho.

```
enfileiradas 2 unidades de trabalho de 256x256
--- matando node-b: SIGKILL, sem leave, sem requeue na saída ---
w-0   node-c    6304   12005696   12012000
w-1   node-c    5637   12008363   12014000
concluidas=2 pendentes=0 falharam=0
```

Log do controller:

```
controller escutando em 0.0.0.0:50051 (graça de heartbeat 9000ms)
nó node-b offline após 9000ms sem heartbeat; 1 unidades em voo reenfileiradas, 0 falharam
trabalho w-0 finalizado em node-c compute=6304us transfer=12005696us total=12012000us checksum=696.515865
trabalho w-1 finalizado em node-c compute=5637us transfer=12008363us total=12014000us checksum=1354.312283
```

`1 unidades em voo reenfileiradas` é a linha que importa: `node-b` segurava uma unidade quando
morreu, a unidade voltou para a fila, e `node-c` a terminou. Um evento de offline para o nó
que realmente foi morto, e nenhum para o nó que estava vivo — o falso reap sumiu. O grafo
drenou sem falhas. `node-b` nunca voltou; o cluster se recuperou sem ele.

O teto de retry e a cascata são cobertos pelos testes unitários
`a_unit_is_failed_once_its_retry_budget_is_spent`,
`failing_a_unit_fails_its_dependents_with_it` e
`a_failed_branch_does_not_fail_unrelated_work`.

## Liveness — heartbeat desacoplado do laço de trabalho

O heartbeat e o health report compartilhavam o laço sequencial do worker com o próprio
trabalho. Uma unidade que rodava mais que a grace do heartbeat, portanto, matava o
heartbeat de fome, e o controller reapava um worker que estava vivo e computando. A unidade
era re-enfileirada, o resultado original era recusado na chegada, e o trabalho era feito
duas vezes — resposta correta, compute desperdiçado, um retry queimado.

O heartbeat agora roda em task própria, com intervalo próprio. A task é parada explicitamente
antes do `LeaveNode`, porque um beat que chegasse depois da partida faria o controller
re-registrar o nó e desfazer a saída.

Isso é visível no teste de falha, que roda uma grace *menor* que o hold de trabalho de
propósito. Antes da correção, a mesma execução logava um worker saudável sendo reapado
repetidamente até uma unidade esgotar os retries:

```
nó node-b offline após 9000ms sem heartbeat; 1 unidades em voo reenfileiradas, 0 falharam
nó node-c offline após 9000ms sem heartbeat; 1 unidades em voo reenfileiradas, 0 falharam
nó node-c offline após 9000ms sem heartbeat; 1 unidades em voo reenfileiradas, 0 falharam
nó node-c offline após 9000ms sem heartbeat; 0 unidades em voo reenfileiradas, 1 falharam
```

Depois da correção, a mesma configuração reapa só o nó que realmente foi morto:

```
nó node-b offline após 9000ms sem heartbeat; 1 unidades em voo reenfileiradas, 0 falharam
```

A grace continua sendo um ajuste: curta demais em relação ao intervalo do heartbeat e um nó
saudável é reapado mesmo assim, então mantenha-a confortavelmente acima do intervalo. O
caminho de saída foi reconferido também — `nó node-b saiu` seguido de `NÓS 0` doze segundos
depois, sem re-registro.

## Teto conhecido

**Nenhum sinal de falha vindo do worker.** Um worker que não consegue executar uma unidade
a descarta em silêncio; o controller só descobre a falha quando o heartbeat do nó para.
Então um worker vivo segurando uma unidade inexequível prende essa unidade em voo até o nó
sair. O orçamento de retry não ajuda, porque nada dispara um hand-back. Fechar isso exige um
campo de falha no resultado, para que "não consegui rodar isto" seja distinguível de "ainda
estou rodando isto".

**O retry não desvia do nó que falhou.** Uma unidade reatribuída vai para o nó que a
reivindicar em seguida. Com um só nó sobrevivente não há para onde ir de qualquer forma;
revisitar com placement, onde uma unidade que falhou num nó não deveria voltar direto para
ele.
