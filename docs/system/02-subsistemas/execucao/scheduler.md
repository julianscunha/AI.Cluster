---
title: "Escalonador"
aliases: ["Scheduler", "Escalonador"]
tags: [sistema/subsistema/execucao]
type: sistema/subsistema
---

# Escalonador

Voltar para [[execucao]].

O escalonador atribui Work Units a nós e a núcleos. Ele roda em dois níveis.

- **Global (controller):** escolha do nó, reservas e prioridade da fila.
- **Local (worker):** quantos núcleos cada unidade recebe e quantas unidades rodam ao mesmo tempo.

## Despacho por demanda

Cada worker pede trabalho ao controller (`RequestWork`) a cada 200 ms quando está ocioso, e logo após concluir uma unidade. O controller decide, naquele pedido, se entrega a unidade àquele nó.

Um nó ocioso que para de pedir trabalho pode segurar uma unidade. Como ele continua enviando heartbeat, não é marcado como offline. Ver limites.

## Filtro de candidatos

Um nó só recebe a unidade se:

- tem threads reportadas em número igual ou maior que `min_cpu_threads` da unidade;
- tem memória reportada suficiente para os operandos e o resultado. Memória zero não filtra;
- é elegível: aprovado e fora dos estados `DRAINING`, `QUARANTINED`, `FAILED` e `OFFLINE`;
- está ocioso (sem unidade em voo) ou tem slot livre.

Um requisito que nenhum nó sobrevivente atende deixa a unidade pendente, sem erro.

## Custo de placement

O custo estimado de rodar a unidade no nó é a soma de três termos, multiplicada pelo fator de saúde. Menor é melhor.

- **Compute:** operações da unidade sobre a taxa do nó (threads vezes score de compute medido). Sem medida, usa 1,0 por thread.
- **Transferência:** bytes que a unidade move sobre a velocidade de rede do nó (menor entre a nominal e a alcançada).
- **Fila:** 50 pontos por unidade já em voo no nó. Impede que um nó ocupado, porém mais rápido, leve tudo.
- **Saúde:** fator multiplicativo. Escore de saúde zero é tratado como não reportado.

Os pesos são fixos. Só a ordem entre nós importa.

## Decisão

O nó que pergunta leva a unidade quando é o candidato ocioso mais barato, ou um candidato dentro da faixa de tolerância (10%). Se outro nó ocioso é mais barato além dessa faixa, o nó que pergunta recebe nada e pede de novo. A unidade fica na fila para o nó que encaixa melhor.

- Só nós ociosos entram no ranking.
- Com `AI_CLUSTER_PLACEMENT=off`, o primeiro nó que pede leva a unidade. Serve para comparar a política com e sem decisão.
- `AI_CLUSTER_PLACEMENT_TOLERANCE` ajusta a faixa. Valor inválido é limitado a uma faixa válida, e faixa zero significa "estritamente o mais barato".

## Prioridade

Cada unidade tem prioridade de 0 a 9 (valores acima de 9 são reduzidos a 9 no enfileiramento). Entre as unidades prontas que cabem no nó que pergunta, sai a de maior `prioridade × 64 + passagens`. Empate sai pela ordem de chegada.

O campo `passagens` conta quantos despachos passaram por cima da unidade enquanto ela esperava. Uma unidade de prioridade baixa ultrapassa um fluxo contínuo de unidades altas depois de cerca de 64 despachos. Uma unidade de prioridade alta que espera por um nó reservado não bloqueia as de prioridade menor.

Enfileirar unidades exige o papel de operador. Quem pode pedir prioridade alta, para cada requisição, ainda não tem regra própria.

## Reservas

Um cliente pode reter um nó para a requisição dele antes de o trabalho existir.

- Uma reserva prende um nó a um `request_id` por um tempo (padrão 60 s, `ttl_ms` configurável). Um nó tem no máximo uma reserva; uma nova substitui a antiga.
- **Reserva parcial:** com `--slots N` a reserva guarda só N dos `work_slots` do nó. Outra requisição ainda usa o nó enquanto `unidades em voo + N < work_slots`; a requisição dona usa sempre. Sem `--slots` (ou `0`) o nó inteiro fica retido, como antes. A unidade é o slot porque é por ele que o placement admite trabalho e divide os núcleos. É contabilidade de admissão no controller: o worker não impõe o limite.
- **Persistência:** as reservas vão no mesmo snapshot da fila (e na réplica I4), então sobrevivem a reinício do controller e são herdadas por um controller que assuma pela cópia de um nó. As vencidas são descartadas ao carregar.
- Enquanto a reserva vale, o nó só aceita unidades daquela requisição, **mesmo com `AI_CLUSTER_PLACEMENT=off`**.
- As unidades da requisição só rodam no nó reservado, mesmo que ele seja o mais caro.
- A expiração é verificada quando alguém pergunta pela reserva, não por um temporizador. Um cliente que some libera o nó no primeiro consulta depois do vencimento.

```bash
ai-cluster reserve <nó> <requisição> --ttl-ms 60000 [--slots 1]
ai-cluster benchmark --request-id <requisição>
ai-cluster release <nó>
ai-cluster reservations
```

## Concorrência local

- Cada worker roda até `AI_CLUSTER_WORK_SLOTS` unidades ao mesmo tempo (padrão 1, limite 64).
- Cada unidade recebe `núcleos do orçamento / slots`, no mínimo 1 núcleo. O orçamento é `min(threads reportadas, núcleos do host)`.
- O nó só aparece como ocupado no controller quando todos os slots têm unidade.

## Limites conhecidos

- Não há rebalanceamento. Trabalho só muda de nó quando o nó sai ou cai.
- Não há execução especulativa.
- Não há filtro por precisão, acelerador ou armazenamento. O worker reporta esses campos, mas nenhuma unidade pode exigi-los.
- Não há fila local ordenada por classe nem admissão por memória no worker. Há só o limite de slots.
- A reserva retém o nó inteiro. Não há reserva de parte de núcleos ou de memória.
- Reservas não sobrevivem a reinício.
- O ranking decide qual nó pega a unidade, mas só a primeira unidade elegível é avaliada para cada nó. Não se escolhe a unidade de melhor encaixe.
- O número de núcleos vem de `available_parallelism`, não de medida de desempenho.
- A prioridade não é persistida. Um reinício zera a idade das unidades.
- Um nó ocioso que para de pedir trabalho segura a unidade enquanto continuar enviando heartbeat.

> Planejado: classes de prioridade nomeadas, dimensões de pontuação de localidade, folga térmica, custo de mover estado e confiabilidade, filtros por precisão e acelerador, fila local por classe, reserva de fração de nó e reservas que sobrevivem a reinício.
