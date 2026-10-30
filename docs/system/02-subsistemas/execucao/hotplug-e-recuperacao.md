---
title: "Hot-plug e recuperação"
aliases: ["Hot-Plug and Recovery", "Hot-plug e recuperação"]
tags: [sistema/subsistema/execucao]
type: sistema/subsistema
---

# Hot-plug e recuperação

Voltar para [[execucao]].

Nós entram e saem do cluster sem operação manual. Trabalho em voo volta à fila sem ser perdido, e o cluster continua andando com os nós que sobraram. Essa é a regra de [[adr-006-hotplug]]: a capacidade muda, e o cluster se reorganiza sozinho.

## Entrada

1. O worker se conecta com o token de entrada e se registra.
2. O controller guarda o nó com a capacidade reportada e o deixa aguardando aprovação do operador (`aic cluster node approve`), a não ser que `AI_CLUSTER_AUTO_APPROVE` esteja ligado.
3. Depois de aprovado, o nó recebe trabalho. Ele já reportou benchmark, saúde e capacidade no registro.

Um nó novo não recebe trabalho antes de ser aprovado. Não há etapa de carga de modelo ou de estado antes de o nó ficar pronto.

## Saída planejada

```text
worker recebe SIGTERM ou Ctrl+C
 -> para de pedir trabalho
 -> espera os slots em execução devolverem seus resultados (teto de 30 s)
 -> para o heartbeat
 -> chama LeaveNode
controller: nó vira DRAINING, sai dos candidatos, unidades em voo voltam à fila,
            nó sai do registro
```

O controller não espera a drenagem do trabalho ativo. As unidades em voo voltam à fila no momento da saída. Como o worker espera seus slots antes de avisar o controller, em geral nada fica em voo nessa hora.

## Queda do nó

```text
heartbeat para
 -> após AI_CLUSTER_HEARTBEAT_GRACE_MS sem heartbeat (padrão 15 000 ms), nó vira OFFLINE
 -> unidades em voo voltam à fila (se tentativas restantes) ou falham com seus dependentes
 -> quando o nó volta a enviar heartbeat, vira READY de novo
```

O controller varre os nós a cada 5 s. Um resultado atrasado do nó caído é recusado, porque a posse da unidade já mudou. Uma unidade reoferecida recomeça do zero.

A grace deve ficar confortavelmente acima do intervalo de heartbeat de 5 s. Uma grace curta demais tira do ar um nó saudável.

## Reinício do controller

- A fila de trabalho, as unidades em voo (com o dono de cada uma) e os resultados ainda necessários são gravados em `AI_CLUSTER_STATE` a cada alteração. A gravação é atômica (arquivo temporário e renomeação). Uma cópia ao lado (`AI_CLUSTER_STATE` com sufixo `.replica`) fica no mesmo volume.
- Ao reiniciar, a fila volta. Uma unidade em voo volta com o mesmo dono. Se o dono reconectar, o resultado que ele estava calculando é aceito.
- Se o dono não reconectar até a grace contada a partir do início do controller, a unidade volta à fila ou falha, como em uma queda de nó.
- Com `AI_CLUSTER_STATE_REPLICA` nos workers, cada worker guarda uma cópia do estado. Se o volume do controller é perdido, o controller adota a cópia que um nó oferece no re-registro, desde que ela seja mais nova que a que ele já tem e ele não esteja com trabalho nas mãos.

Quando o controller perde o estado do disco e nenhum worker tem cópia, a fila se perde.

## Revogação

`aic cluster node revoke <nó>` tira a confiança do nó e devolve as unidades em voo dele à fila. Ver [[saude-e-confianca]].

## Garantias

- Um resultado é aceito só do nó para o qual a unidade foi despachada. Resultado atrasado de nó que perdeu a posse é recusado.
- Cada atribuição conta uma tentativa. Quando um nó cai ou sai, a unidade volta à fila, e a próxima atribuição conta de novo.
- O orçamento de três tentativas limita o desperdício quando um nó continua caindo.
- Com o estado gravado em disco, uma unidade em voo volta ao reinício com o mesmo dono. Sem cópia do estado, um reinício perde as unidades não concluídas, e o cliente deve reenviá-las.

## Limites conhecidos

- Não há drenagem que espere o trabalho ativo terminar. Quando um nó sai, suas unidades em voo voltam à fila na hora.
- Não há checkpoint de trabalho ativo nem liberação de modelo ou estado na saída.
- A perda de nó é notada só pelo silêncio do heartbeat. Não há sinal de falha vindo do worker.
- Não há estado `FAILED` para nó que parou de responder. Usa-se `OFFLINE`.
- A gravação da posse é preguiçosa: a atribuição de uma unidade não é gravada sozinha, só as mutações seguintes levam a posse ao disco. Se o controller reinicia logo após um despacho, a unidade volta à fila sem dono e pode rodar duas vezes. O resultado do primeiro nó é recusado quando chega, e o trabalho extra é gasto. Para uma multiplicação de matrizes determinística, o resultado final é o mesmo.
- Não há replanejamento com comparação de melhoria esperada. A reavaliação do cluster acontece a cada pedido de trabalho.
- Não há capacidade agregada atualizada após entrada e saída.
- Uma unidade reoferecida recalcula a multiplicação inteira. Não há recuperação incremental.
- A reatribuição não evita o nó que já falhou a unidade.
- A cópia de estado fica em cada nó com `AI_CLUSTER_STATE_REPLICA`, sem fator de replicação nem nó designado. Com estado grande, o tráfego cresce com o número de nós e de gravações.
- A cópia não tem identidade do cluster. Um controller novo apontado para nós que guardam a cópia de outro cluster adota aquela fila.
- O registro de nós e o log de eventos não são persistidos por padrão. Eventos só sobrevivem a reinício quando `AI_CLUSTER_EVENTS` está definido, e guardam os 256 últimos eventos. O arquivo é compactado a partir de 1024 linhas. Não há `fsync` a cada evento: uma queda de energia pode perder os últimos.

> Planejado: drenagem que espera o trabalho ativo, checkpoint de trabalho ativo, estado `FAILED`, replanejamento com ganho esperado, aquecimento de modelo e de estado antes de `READY` (ver [[model-manager]]), e recuperação incremental. Ver também [[state-manager]].
