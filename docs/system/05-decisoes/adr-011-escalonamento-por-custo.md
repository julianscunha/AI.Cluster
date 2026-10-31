---
title: "ADR-011 — Escalonamento por custo, prioridade e quarentena"
aliases: ["ADR-011", "Escalonamento por custo"]
tags: [sistema/adr, subsistema/execucao]
type: sistema/adr
---

# ADR-011 — Escalonamento por custo, prioridade e quarentena

Status: decidido · Data: 2026-10-25 · Voltar para [[decisoes]].

## Contexto

O controller entregava a próxima unidade a quem pedisse primeiro. Num cluster heterogêneo isso põe trabalho pesado no worker fraco enquanto o forte espera. Além disso a fila era FIFO, o worker rodava uma unidade por vez e o controller aceitava qualquer resultado de um worker aprovado.

## Decisão

1. **O worker pergunta, o controller decide.** O modelo é de puxar: o worker pede trabalho e `placement::decide` responde se ele é o melhor lugar para a unidade. Se não for, a unidade espera pelo worker que cabe melhor, em vez de ir para quem perguntou primeiro.
2. **Filtro duro, depois custo.** `min_cpu_threads` e a memória que a unidade ocupa eliminam candidatos. Entre os que restam, o custo soma compute (flops sobre `threads × compute_score`), transferência (bytes sobre a taxa do link, a menor entre a nominal e a medida), fila e um fator de saúde (`HEALTH_WEIGHT`). Os pesos são constantes iguais a 1, exceto `QUEUE_WEIGHT`.
3. **Só candidatos com slot livre competem.** Um worker ocupado não está pedindo, então não pode segurar a unidade. "Ocupado" é ter todos os `work_slots` em uso.
4. **Histerese.** O worker que pergunta só cede se outro for mais barato além de uma banda (10%, `AI_CLUSTER_PLACEMENT_TOLERANCE`). Ceder custa uma espera, e diferença de arredondamento não paga isso.
5. **Reserva vence custo.** Um worker reservado para uma requisição é o único lar das unidades dela e não recebe as de outras.
6. **Prioridade na fila do controller.** Cada unidade tem uma classe inteira (`priority`, de 0 a 9). Maior sai primeiro, FIFO dentro da classe, e cada despacho que passa por cima de uma unidade soma um ponto de idade; uma classe vale 64 pontos, então a classe baixa não morre de fome. `claim_placed` percorre as unidades por ranking e leva a primeira que `decide` aceita, para uma unidade alta que espera por outro worker não travar as baixas.
7. **Fila local por recurso no worker.** O worker roda até `AI_CLUSTER_WORK_SLOTS` unidades ao mesmo tempo (padrão 1), divide os núcleos entre elas e só pede trabalho com um slot livre. As remedições de capability só rodam com o worker vazio.
8. **Confiança por evidência.** O controller confere forma, números finitos e checksum de cada resultado. Três inválidos seguidos põem o worker em `QUARANTINED`, que persiste junto da credencial e só `approve` desfaz. A unidade devolvida não gasta a tentativa dela: a culpa é do worker.

## Consequências

- Em cluster heterogêneo, o placement por custo evita que o worker fraco pegue o trabalho pesado. A diferença medida foi de ordens de grandeza em relação a primeiro-a-pedir.
- Um worker que perde a disputa espera até um intervalo de poll (200 ms). Em unidades curtas esse custo é real; por isso a banda de histerese.
- A prioridade é um número, não classes nomeadas, e só o operador a define.
- Um slot a mais esconde latência de transferência, mas divide os núcleos: com unidades que já ocupam o worker inteiro, 1 slot é melhor.

## Alternativas

- Round-robin ou primeiro-que-pergunta: descartado pela medição de que a espera fica muito maior.
- Empurrar do controller para o worker: exigiria o controller conhecer a ocupação em tempo real; o modelo de puxar a dá de graça.
- Prioridade estrita sem envelhecimento: simples, mas uma corrente de unidades altas mata as baixas.
- Excluir o worker por qualquer resultado inválido: um erro isolado (corrupção de rede, bug transitório) não justifica tirar o worker; três seguidos justificam.

## Limites

- Locality, folga térmica, custo de mover estado e prazo não entram no custo.
- Não há rebalanceamento de trabalho em voo nem execução especulativa.
- O checksum prova coerência interna, não a conta certa: um worker que mente de forma consistente passa. Não existe conferência por réplica.
- Planejado: níveis de confiança `UNKNOWN`, `BOOTSTRAPPED` e `VALIDATED`, dependentes de enrollment ([[scheduler]], [[saude-e-confianca]]).
