---
title: "ADR-027 — Worker removido só volta por convite"
aliases: ["ADR-027", "worker removido", "lápide de remoção", "remove e convite"]
tags: [sistema/adr, subsistema/seguranca, subsistema/plataforma]
type: sistema/adr
---

# ADR-027 — Worker removido só volta por convite

Status: decidido · Data: 2026-10-31 · Voltar para [[decisoes]].

## Contexto

`aic cluster node remove` esquecia o worker por inteiro: registro e credencial. O processo do worker, ainda vivo e com a credencial guardada, se registrava de novo na hora como worker novo e pendente (`node_registered` logo depois de `node_removed`). O operador esperava que o worker sumisse e só voltasse por um convite (`aic worker join`).

## Decisão

1. **Remover deixa uma lápide.** `forget` guarda a credencial do worker com a marca `removed` no arquivo de estado (sexta coluna). A lápide não tem confiança, não tem chave fixada (ADR-026) e não tem registro vivo.
2. **A credencial removida é recusada.** `RegisterNode` com ela responde `PermissionDenied` com `NODE_REMOVED_MESSAGE`, e toda chamada de nó (`authorize_node`) também. Um registro sem credencial, isto é, um convite novo, entra como pendente com credencial nova, e a lápide sai.
3. **O worker recusado não entra em laço.** Ele imprime uma linha e confere de novo com backoff (30 s, dobrando até 10 min). Se for removido enquanto roda, para de pedir trabalho e encerra depois do que está em voo. O systemd o reinicia uma vez e ele cai nesse backoff.
4. **O convite apaga a credencial guardada.** O `bootstrap.sh` com `AIC_INVITED=1` (o script do convite) remove `state/credential`. `aic worker change` já a apagava. Rodar o `bootstrap.sh` sem convite (atualização) a mantém.
5. **Retirar mantém a identidade.** `retire` não põe lápide. O worker aposentado segue conhecido, pendente, com a credencial válida; se volta com ela, é o mesmo worker, e `approve` com a impressão de antes o devolve ao trabalho.
6. **Revogar não põe lápide** (segue ADR-026 e a regra de rotação do S5). A credencial antiga deixa de valer; o worker vivo encerra com `Unauthenticated`; reiniciado, entra como worker novo pendente com credencial nova. Quem quer barrar esse retorno remove.

## Consequências

- **Reinstalar o controller, ou perder o arquivo de estado (`AI_CLUSTER_NODES`), apaga as lápides.** Um worker antigo que se registra com a credencial antiga entra como pendente. Por isso, depois de reinstalar o controller, cada worker antigo deve reentrar com um convite (`aic worker join`), que apaga a credencial guardada.
- Convite para uma máquina que ainda é worker aprovado com o mesmo id é recusado pelo controller (`já registrado`). Tire-a antes com `revoke` ou `remove`.
- A lápide guarda a credencial, por isso o arquivo de estado segue 0600 como antes.

## Alternativas

- **Recusar pelo id, sem credencial:** descartada. Um convite novo, sem credencial, precisa entrar.
- **Sair do processo com código que o systemd não reinicia (`RestartPreventExitStatus`):** descartada. Exige unit nova em todas as instalações; o backoff do próprio worker funciona com a unit atual.
- **Revogar também com lápide:** adiada. A regra de que o worker revogado volta pendente é testada (S5) e é a que ADR-026 usa. Fica como decisão em aberto.

## Limites

- Sem o arquivo de estado não há lápide (ver Consequências). Não há como recusar um worker que o controller não lembra.
- Revogar não barra o retorno com a credencial antiga (ver Alternativas).
- Testado por unidade (registro, serviço e persistência). O fluxo ponta a ponta (remover, worker recusado, convite, aprovação) não foi exercitado em processos reais nem em hardware. O `bootstrap.sh` com convite não tem teste de shell.

Ver também [[adr-026-renovacao-e-revogacao-do-certificado-do-no]] e [[adr-023-instalacao-com-aprovacao-do-controller]].
