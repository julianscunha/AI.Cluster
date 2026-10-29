---
title: "Storage Intelligence"
aliases: ["Storage Intelligence", "Inteligência de armazenamento"]
tags: [sistema/subsistema/dados]
type: sistema/subsistema
---

# Storage Intelligence

O armazenamento é tratado como recurso complementar à memória e à execução. Hoje o cluster só classifica os discos de cada nó e mede sua velocidade de escrita. Nenhuma decisão de alocação usa essa informação. Voltar para [[dados]].

## Classificação

Cada worker lê `/sys/block/*/queue/rotational` e o nome do dispositivo:

| Classe | Critério | Camada de estado ([[state-manager]]) |
|---|---|---|
| `nvme` | Nome com prefixo `nvme` | WARM |
| `ssd` | Dispositivo sem rotação e nome sem prefixo `nvme` | WARM |
| `hdd` | Dispositivo com rotação | COLD |

A RAM é HOT e já aparece na memória do nó. Não entra na classificação de armazenamento.

Dispositivos `loop`, `ram` e device-mapper são ignorados, porque não são armazenamento usável pelo cluster.

O relatório de capacidade traz `storage_classes` da classe mais quente para a mais fria. Um nó sem disco visível reporta lista vazia, que significa "não medido" e não "sem armazenamento".

`AI_CLUSTER_STORAGE_CLASSES` substitui a detecção, e `ai-cluster nodes` mostra a coluna `ARMAZ`.

## Medida de escrita

O worker escreve 16 MiB com `fsync` no primeiro diretório que não seja RAM entre o temporário, `/var/tmp` e o diretório de trabalho do worker (ou em `AI_CLUSTER_STORAGE_PROBE_DIR`; na imagem do Orange Pi `/tmp` e `/var/tmp` são tmpfs, e antes disso o cartão SD saía com score de RAM), pega a melhor de três execuções e relaciona a taxa a 500 MB/s (um SSD SATA). Também mede a latência de escrita de 4 KiB com `fsync`. A medida é remedida no ocioso.

O `fsync` faz a medida refletir o disco, e não o cache de página.

A saúde de armazenamento usada pelo escore de saúde (ver [[saude-e-confianca]]) é a razão entre a taxa atual e a taxa de registro. `AI_CLUSTER_STORAGE_HEALTH` fixa esse valor.

## Limites conhecidos

- NVMe e SSD SATA ficam juntos na mesma camada, porque a classificação não usa latência nem vazão de leitura.
- A classificação diz quais classes o nó tem, não quanto espaço há em cada uma. Um nó com dois NVMe reporta só `nvme`.
- Não há medida de vazão de leitura, nem de capacidade por dispositivo, nem de contagem de dispositivos.
- A medida de escrita é só do disco do diretório temporário, não de cada classe.
- Dentro de contêiner, `/sys/block` mostra os discos do host.
- Não há SMART, temperatura de disco nem medida de desgaste ou erros.
- Nada no cluster grava estado em disco por camada. Os pesos dos modelos são lidos do disco sem política de camada. O cache de fatias é só RAM, com despejo LRU.
- Não há reserva de espaço para sistema, swap, cache, checkpoint ou emergência.

> Planejado: alocação de espaço por camada (sistema, swap, cache de modelo, cache de tensores, checkpoint, reserva de emergência), política de SSD e NVMe para swap e cache conforme a saúde, redução de escrita e alerta ao controller em degradação, e cache de modelo HOT, WARM e COLD (ver [[model-manager]]).
