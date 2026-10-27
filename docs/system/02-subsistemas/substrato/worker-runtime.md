---
title: "Runtime do worker"
aliases: ["Runtime do worker", "Worker Runtime"]
tags: [sistema/subsistema/substrato]
type: sistema/subsistema
---

# Runtime do worker

O worker é o processo que roda em cada máquina do cluster. Ele recebe Work Units do controller, executa-as com o hardware local, reporta capacidade e saúde, e mantém os recursos que o controller pedir (fatias de peso, cópia de estado).

## Responsabilidades

- pedir unidades ao controller e devolver o resultado com checksum;
- medir e reportar capacidade e saúde ([[capability-engine]], [[saude-e-confianca]]);
- manter a cópia do estado do controller, quando configurado;
- manter o canal residente de fatias de peso, quando configurado.

## Ciclo de trabalho

1. O worker pede uma unidade ao controller (`RequestWork`).
2. Sem unidade, espera 200 ms e pede de novo. Depois de concluir uma, pede de novo na hora.
3. Executa a unidade com os núcleos concedidos e devolve o resultado (`SubmitWorkResult`) com checksum e tempo de cálculo.
4. Um heartbeat separado roda a cada 5 s, em tarefa própria. Uma unidade longa não deixa o nó parecer morto.

O worker valida o tamanho das matrizes antes de multiplicar. Uma unidade incoerente volta marcada como inválida, e o controller não tenta outro nó por causa dela.

## Execução local

- **Concorrência.** `AI_CLUSTER_WORK_SLOTS` define quantas unidades rodam ao mesmo tempo (padrão 1, limite 64). O worker só pede trabalho quando há slot livre, e segura o slot até entregar o resultado.
- **Núcleos.** O orçamento é `min(threads reportadas, núcleos do host)`, dividido entre os slots (no mínimo 1 núcleo por unidade). O produto de matrizes é dividido por linhas de saída, e o resultado é igual, bit a bit, ao do caminho serial. O checksum não muda com o número de threads.
- **Remedições.** Medidas de compute, armazenamento e rede só rodam com o nó vazio, para não roubar núcleos de unidade em voo.
- **Falha de execução.** Se o cálculo entra em pânico ou o resultado não chega ao controller após três tentativas, o worker sai do cluster. Sua unidade volta à fila pelo caminho de saída.

## Canal residente de fatias

Usado só por inferência, e só quando o worker recebe `AI_CLUSTER_MODEL_DIR`.

- O worker abre um stream `ShardChannel` com o controller, identificando-se pelo `node_id`. Se a conexão cai ou é recusada, tenta de novo a cada 2 s.
- As fatias de peso ficam no worker. Só ativações e saídas cruzam a rede. Os pesos não saem do disco do nó.
- O cache de fatias tem teto de `AI_CLUSTER_SHARD_CACHE_MB` (padrão 2048 MiB), com despejo LRU.
- Até 2 cálculos em paralelo, fila de resposta de 64 quadros.
- Só tensores de projeção de camada são lidos do disco. Caminho fora da raiz do modelo é recusado. Uma fatia tem no máximo 256 MiB de entrada e a saída no máximo 64 MiB.

Detalhes do protocolo em [[canal-de-fatias]].

## Cópia do estado do controller

Com `AI_CLUSTER_STATE_REPLICA` definido, o worker guarda uma cópia do estado de trabalho do controller. A cópia chega junto de um heartbeat, quando a época do controller avançou além da que o nó tem. Ela é reoferecida no re-registro. Ver [[state-manager]].

## Variáveis de ambiente

| Variável | Padrão | Efeito |
|---|---|---|
| `AI_CLUSTER_CONTROLLER` | Endereço padrão do controller | Endereço do controller. |
| `AI_CLUSTER_NODE_ID` | Conteúdo de `/etc/hostname` | Identidade do nó. |
| `AI_CLUSTER_NODE_CREDENTIAL_FILE` | | Arquivo com a credencial do nó. |
| `AI_CLUSTER_WORK_SLOTS` | 1 (limite 1 a 64) | Unidades concorrentes. |
| `AI_CLUSTER_MODEL_DIR` | | Raiz dos modelos. Liga o canal de fatias. |
| `AI_CLUSTER_SHARD_CACHE_MB` | 2048 | Teto do cache de fatias. |
| `AI_CLUSTER_STATE_REPLICA` | | Arquivo onde o nó guarda a cópia do estado do controller. |
| `AI_CLUSTER_RECOMPUTE_INTERVAL_SECS` | 60 | Cadência das remedições no ocioso. |

## Prioridade

O worker não tem fila de prioridade própria. A prioridade vive na fila do controller (inteiro de 0 a 9, ver [[scheduler]]).

## Limites conhecidos

- Só há backend de CPU. Não há execução em GPU, ROCm, Vulkan, OpenCL nem NPU.
- O worker roda uma unidade por slot, sem fila local ordenada por classe e sem admissão por memória. A única decisão local é quantos núcleos usar.
- O worker não guarda resultados nem faz checkpoint do próprio trabalho. A cópia de estado é do controller.
- Uma unidade reoferecida recomeça do zero.
- Não há controle de memória por execução.

> Planejado: backends de GPU e NPU, Memory Manager com teto por execução, Local Scheduler com decisão de núcleo, GPU e buffer, classes de prioridade nomeadas (`CRITICAL` até `CACHE`), cache de resultados, checkpoint próprio do worker, telemetria local, zero-copy e DMA, e decisão entre recomputar e transferir.
