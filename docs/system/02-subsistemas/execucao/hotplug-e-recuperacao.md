---
title: "Hot-plug e recuperação"
aliases: ["Hot-Plug and Recovery", "Hot-plug e recuperação"]
tags: [sistema/subsistema/execucao]
type: sistema/subsistema
---

# Hot-plug e recuperação

Voltar para [[execucao]].

Workers entram e saem do cluster sem operação manual. Trabalho em voo volta à fila sem ser perdido, e o cluster continua andando com os workers que sobraram. Essa é a regra de [[adr-006-hotplug]]: a capacidade muda, e o cluster se reorganiza sozinho.

## Entrada

1. O worker se conecta com o token de entrada e se registra.
2. O controller guarda o worker com a capacidade reportada e o deixa aguardando aprovação do operador (`aic cluster node approve`), a não ser que `AI_CLUSTER_AUTO_APPROVE` esteja ligado.
3. Depois de aprovado, o worker recebe trabalho. Ele já reportou benchmark, saúde e capacidade no registro.

Um worker novo não recebe trabalho antes de ser aprovado. Não há etapa de carga de modelo ou de estado antes de o worker ficar pronto.

## Saída planejada

```text
worker recebe SIGTERM ou Ctrl+C
 -> para de pedir trabalho
 -> espera os slots em execução devolverem seus resultados (teto de 30 s)
 -> para o heartbeat
 -> chama LeaveNode
controller: worker vira DRAINING, sai dos candidatos, unidades em voo voltam à fila,
            worker sai do registro
```

O controller não espera a drenagem do trabalho ativo. As unidades em voo voltam à fila no momento da saída. Como o worker espera seus slots antes de avisar o controller, em geral nada fica em voo nessa hora.

## Queda do worker

```text
heartbeat para
 -> após AI_CLUSTER_HEARTBEAT_GRACE_MS sem heartbeat (padrão 15 000 ms), worker vira OFFLINE
 -> unidades em voo voltam à fila (se tentativas restantes) ou falham com seus dependentes
 -> quando o worker volta a enviar heartbeat, vira READY de novo
```

O controller varre os workers a cada 5 s. Um resultado atrasado do worker caído é recusado, porque a posse da unidade já mudou. Uma unidade reoferecida recomeça do zero.

A grace deve ficar confortavelmente acima do intervalo de heartbeat de 5 s. Uma grace curta demais tira do ar um worker saudável.

## Reinício do controller

- A fila de trabalho, as unidades em voo (com o dono de cada uma) e os resultados ainda necessários são gravados em `AI_CLUSTER_STATE` a cada alteração. A gravação é atômica (arquivo temporário e renomeação). Uma cópia ao lado (`AI_CLUSTER_STATE` com sufixo `.replica`) fica no mesmo volume.
- Ao reiniciar, a fila volta. Uma unidade em voo volta com o mesmo dono. Se o dono reconectar, o resultado que ele estava calculando é aceito.
- Se o dono não reconectar até a grace contada a partir do início do controller, a unidade volta à fila ou falha, como em uma queda de worker.
- Com `AI_CLUSTER_STATE_REPLICA` nos workers, cada worker guarda uma cópia do estado. Se o volume do controller é perdido, o controller adota a cópia que um worker oferece no re-registro, desde que ela seja mais nova que a que ele já tem e ele não esteja com trabalho nas mãos.

Quando o controller perde o estado do disco e nenhum worker tem cópia, a fila se perde.

## Revogação

`aic cluster node revoke <worker>` tira a confiança do worker e devolve as unidades em voo dele à fila. Ver [[saude-e-confianca]].

## Garantias

- Um resultado é aceito só do worker para o qual a unidade foi despachada. Resultado atrasado de worker que perdeu a posse é recusado.
- Cada atribuição conta uma tentativa. Quando um worker cai ou sai, a unidade volta à fila, e a próxima atribuição conta de novo.
- O orçamento de três tentativas limita o desperdício quando um worker continua caindo.
- Com o estado gravado em disco, uma unidade em voo volta ao reinício com o mesmo dono. Sem cópia do estado, um reinício perde as unidades não concluídas, e o cliente deve reenviá-las.

## Limites conhecidos

- Não há drenagem que espere o trabalho ativo terminar. Quando um worker sai, suas unidades em voo voltam à fila na hora.
- Não há checkpoint de trabalho ativo nem liberação de modelo ou estado na saída.
- A perda de worker é notada só pelo silêncio do heartbeat. Não há sinal de falha vindo do worker.
- Não há estado `FAILED` para worker que parou de responder. Usa-se `OFFLINE`.
- A gravação da posse é preguiçosa: a atribuição de uma unidade não é gravada sozinha, só as mutações seguintes levam a posse ao disco. Se o controller reinicia logo após um despacho, a unidade volta à fila sem dono e pode rodar duas vezes. O resultado do primeiro worker é recusado quando chega, e o trabalho extra é gasto. Para uma multiplicação de matrizes determinística, o resultado final é o mesmo.
- Não há replanejamento com comparação de melhoria esperada. A reavaliação do cluster acontece a cada pedido de trabalho.
- Não há capacidade agregada atualizada após entrada e saída.
- Uma unidade reoferecida recalcula a multiplicação inteira. Não há recuperação incremental.
- A reatribuição não evita o worker que já falhou a unidade.
- A cópia de estado fica em cada worker com `AI_CLUSTER_STATE_REPLICA`, sem fator de replicação nem worker designado. Com estado grande, o tráfego cresce com o número de workers e de gravações.
- A cópia não tem identidade do cluster. Um controller novo apontado para workers que guardam a cópia de outro cluster adota aquela fila.
- O registro de workers e o log de eventos não são persistidos por padrão. Eventos só sobrevivem a reinício quando `AI_CLUSTER_EVENTS` está definido, e guardam os 256 últimos eventos. O arquivo é compactado a partir de 1024 linhas. Não há `fsync` a cada evento: uma queda de energia pode perder os últimos.

> Planejado: drenagem que espera o trabalho ativo, checkpoint de trabalho ativo, estado `FAILED`, replanejamento com ganho esperado, aquecimento de modelo e de estado antes de `READY` (ver [[model-manager]]), e recuperação incremental. Ver também [[state-manager]].
