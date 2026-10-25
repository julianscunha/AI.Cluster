---
title: "F03 — Tolerância a falhas"
aliases: ["phase-2/fault-tolerance", "F4"]
tags: [implementacao/fase/03, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-03-execucao-distribuida]]"
codigos-antigos: ["F4", "roadmap-2", "M1"]
sistema: ["[[hotplug-e-recuperacao]]", "[[execution-graph]]"]
evidencia: ["code/tests/phase-2-failure.sh"]
---
# F03 — Tolerância a falhas

- [ ] Superado

## Objetivo

Reatribuição limitada: uma unidade cujo worker morre é entregue a outro nó, mas só algumas vezes. Passado o orçamento, a unidade falha, e tudo a jusante dela também, para que o grafo sempre chegue a uma resposta em vez de travar.

## O que foi feito

- `WorkStore` conta tentativas de dispatch por work id. O `claim` incrementa o contador; a contagem sobrevive a um hand-back porque vive no store, não no registro de dispatch descartado.
- `requeue_for_node` devolve uma unidade só enquanto o orçamento dura. Com `MAX_ATTEMPTS = 3` (uma primeira tentativa mais duas reatribuições), a unidade falha em vez de voltar à fila.
- `fail_cascade` falha os dependentes junto com a unidade, transitivamente. Um dependente espera pelo *output* da origem, e uma unidade falha nunca produz um; sem a cascata o dependente ficaria na fila para sempre e `pending()` nunca chegaria a zero.
- `RequeueOutcome` reporta as duas contagens, para que o caminho de saída e o sweep de offline logem `requeued N, failed M`.
- Heartbeat e health report do worker rodam em task própria, com intervalo próprio (ver [Liveness](#liveness--heartbeat-desacoplado-do-laço-de-trabalho)).
- `WorkReportResponse.failed` expõe a contagem de falhas; o CLI imprime `failed=`.
- Hook de teste `AI_CLUSTER_WORK_DELAY_MS`: segura uma unidade reivindicada antes de computá-la. Sem a variável, sem atraso. Existe para que um teste de falha mate um worker enquanto ele realmente segura trabalho.

## Decisões

- **Tentativas contadas no claim.** O número significa "vezes que esta unidade foi iniciada", não "vezes que ela voltou". Uma unidade que completa zera o contador.
- **Unidade falha sai de fila e de voo.** `pending()` exclui unidades falhas. Um cliente que faz poll esperando conclusão termina em um de dois desfechos (drenou ou desistiu), em vez de esperar pelo próprio deadline. `failed=0` significa que o grafo rodou até o fim.
- **Resultado atrasado de worker morto é recusado.** O registro de dispatch é descartado ao esgotar o orçamento, então o resultado tardio não é aceito.
- **Cascata não alcança unidade em voo.** A prontidão retém um dependente até que a origem tenha produzido output, então nada a jusante de uma falha chega a ser despachado.

## Evidência

Reproduzir com `bash code/tests/phase-2-failure.sh`. Duas unidades enfileiradas, cada uma segurada 12s (`AI_CLUSTER_WORK_DELAY_MS`) para que o kill caia com ela em voo. A grace é posta em 9s, de propósito menor que o hold, para que um worker saudável fosse reapado se o heartbeat ainda compartilhasse o laço de trabalho. `node-b` leva `docker kill`, sem chamada de leave.

```text
enfileiradas 2 unidades de trabalho de 256x256
--- matando node-b: SIGKILL, sem leave, sem requeue na saída ---
w-0   node-c    6304   12005696   12012000
w-1   node-c    5637   12008363   12014000
concluidas=2 pendentes=0 falharam=0
```

Log do controller:

```text
controller escutando em 0.0.0.0:50051 (graça de heartbeat 9000ms)
nó node-b offline após 9000ms sem heartbeat; 1 unidades em voo reenfileiradas, 0 falharam
trabalho w-0 finalizado em node-c compute=6304us transfer=12005696us total=12012000us checksum=696.515865
trabalho w-1 finalizado em node-c compute=5637us transfer=12008363us total=12014000us checksum=1354.312283
```

`1 unidades em voo reenfileiradas` é a linha que importa: `node-b` segurava uma unidade ao morrer, a unidade voltou à fila e `node-c` a terminou. Um evento de offline para o nó que foi morto e nenhum para o nó vivo. O grafo drenou sem falhas; `node-b` nunca voltou.

Cobertura unitária: `a_unit_is_failed_once_its_retry_budget_is_spent`, `failing_a_unit_fails_its_dependents_with_it` e `a_failed_branch_does_not_fail_unrelated_work`.

> [!example] Liveness antes e depois da correção
> Antes, na mesma configuração, o log mostrava um worker saudável sendo reapado repetidamente até uma unidade esgotar os retries:
> ```text
> nó node-b offline após 9000ms sem heartbeat; 1 unidades em voo reenfileiradas, 0 falharam
> nó node-c offline após 9000ms sem heartbeat; 1 unidades em voo reenfileiradas, 0 falharam
> nó node-c offline após 9000ms sem heartbeat; 1 unidades em voo reenfileiradas, 0 falharam
> nó node-c offline após 9000ms sem heartbeat; 0 unidades em voo reenfileiradas, 1 falharam
> ```
> Depois, só o nó realmente morto é reapado: `nó node-b offline após 9000ms sem heartbeat; 1 unidades em voo reenfileiradas, 0 falharam`. A saída graciosa também foi reconferida: `nó node-b saiu` seguido de `NÓS 0` doze segundos depois, sem re-registro.

## Liveness — heartbeat desacoplado do laço de trabalho

O heartbeat e o health report compartilhavam o laço sequencial do worker com o próprio trabalho. Uma unidade que rodava mais que a grace matava o heartbeat de fome, e o controller reapava um worker vivo e computando. A unidade era re-enfileirada, o resultado original era recusado na chegada, e o trabalho era feito duas vezes: resposta correta, compute desperdiçado, um retry queimado.

O heartbeat agora roda em task própria, com intervalo próprio. A task é parada explicitamente antes do `LeaveNode`, porque um beat que chegasse depois da partida faria o controller re-registrar o nó e desfazer a saída.

> [!warning] Grace é um ajuste
> Grace curta demais em relação ao intervalo do heartbeat reapa um nó saudável mesmo assim. Mantê-la confortavelmente acima do intervalo.

## Limites

> [!warning] Nenhum sinal de falha vindo do worker
> Um worker que não consegue executar uma unidade a descarta em silêncio; o controller só descobre quando o heartbeat do nó para. Um worker vivo segurando uma unidade inexequível prende essa unidade em voo até o nó sair. O orçamento de retry não ajuda, porque nada dispara um hand-back. Fechar isso exige um campo de falha no resultado, para distinguir "não consegui rodar isto" de "ainda estou rodando isto".

> [!warning] Retry não desvia do nó que falhou
> Uma unidade reatribuída vai para o nó que a reivindicar em seguida. Com um só nó sobrevivente não há para onde ir; revisitar com placement, onde uma unidade que falhou num nó não deveria voltar direto para ele.

## Próximos passos

- Campo de falha no resultado do worker (falha explícita de execução).
- Evitar o nó que já falhou a unidade, no placement.
