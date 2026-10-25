---
title: "F10 — Classificação de armazenamento por nó"
aliases: ["I2", "storage-tiers", "Classificação de armazenamento por nó", "K1", "K2"]
tags: [implementacao/fase/10, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-10-inteligencia-de-storage]]"
codigos-antigos: ["I2", "K1", "K2"]
sistema: ["[[storage-intelligence]]", "[[state-manager]]"]
evidencia: ["code/tests/phase-i-storage.sh", "code/tests/phase-c-network.sh"]
---
# F10 — Classificação de armazenamento por nó

- [ ] Superado

## Objetivo

O nó reportava a RAM e nada sobre disco, então o cluster não sabia em qual camada de estado um nó podia guardar dados. Esta fatia faz cada nó dizer quais classes de armazenamento tem, e mede a vazão de escrita do disco do diretório temporário.

## O que foi feito

- `CapabilityReport.storage_classes` no proto: as classes em bloco detectadas no nó, da mais quente para a mais fria. O campo antigo `storage_score` (um `double` que nunca era preenchido) continua para o score de escrita. Um escalar não carrega *quais* camadas existem, por isso a classificação virou lista.
- `storage_class`, `storage_classes_from` e `storage_classes` no worker leem `/sys/block/*/queue/rotational` e traduzem em classe.
- Benchmark de escrita: o worker escreve 16 MiB com `fsync` (melhor de 3) no diretório temporário e reporta a taxa em `storage_score`, relativa a 500 MB/s (um SATA SSD). 1.0 é isso, ~0.2 é um HDD, ~6 é um NVMe. O `fsync` faz o número ser do disco; sem ele, o page cache responde.
- `ai-cluster nodes` ganhou as colunas `ARMAZ` (classes) e `ARMAZ_SCORE`. O detalhe do nó mostra `armaz_score=`.

Sem dependência nova: `/sys/block` é um `read_dir` e um `read_to_string`, o mesmo tipo de leitura que o worker já faz em `/proc`.

## Decisões

- **Heurística por nome.** O kernel responde `rotational`: 0 é estado sólido, 1 é disco giratório. Isso separa HDD de SSD, mas não separa NVMe de SATA SSD, que respondem 0 igualmente. O nome do dispositivo é a única pista sem abrir o dispositivo, então `nvme` vem do prefixo. Não é medida de latência: NVMe e SATA SSD ficam na mesma camada WARM até existir benchmark de storage.
- **Loop, ram e device-mapper ficam de fora.** Existem em qualquer nó e não são armazenamento usável pelo cluster.
- **Por dispositivo, não por camada.** HDD é COLD, SSD e NVMe são WARM. HOT é RAM, que já vem em `memory_bytes`; duplicar aqui criaria dois números para a mesma coisa.
- **Vazio é "não medido", nunca "sem armazenamento".** Nó sem `/sys/block` reporta lista vazia, e a CLI imprime `-`. Zero ali diria que o valor é zero.
- **Override por ambiente.** `AI_CLUSTER_STORAGE_CLASSES` fixa as classes de um nó, como `AI_CLUSTER_CPU_THREADS` e `AI_CLUSTER_MEMORY_BYTES`. Sem ele, a resposta é do hardware do host.
- **Benchmark sem consumidor.** Nenhuma unidade de trabalho toca disco hoje (os operandos passam pelo controller), então um termo de placement seria sempre zero ou inventado. O score fica só na CLI.

## Evidência

> [!example] `phase-i-storage.sh` e `phase-c-network.sh` (A)
> Três nós: um com classes fixadas, um sem override e um com override vazio.
>
> ```
> NÓ                   ESTADO     ARQ        THREADS    RAM_GB COMPUTE    SAÚDE    CPU_%  TEMP_C  SWAP_% ARMAZ
> node-blank           READY      x86_64           4      3.77    1.19     0.74     2.10    0.00    1.14 -
> node-fixed           READY      x86_64           4      3.77    1.16     0.74     1.45    0.00    1.14 nvme,ssd
> node-real            READY      x86_64           4      3.77    1.17     0.74     1.40    0.00    1.14 hdd
> ```
>
> `node-real` é a detecção real: no contêiner o host apresenta os discos como `rotational=1`, então a resposta honesta é `hdd`. Em `phase-c-network.sh` (A), `armaz_score=0.06`, que é o overlay do Docker Desktop e não o disco do host.

- E2E: `bash code/tests/phase-i-storage.sh`.
- Testes de unidade em `worker/src/main.rs`: `classifies_block_devices_by_rotation_and_name` e `storage_classes_are_distinct_and_hottest_first`.

## Limites e tetos

> [!warning] Sem medida de latência ou vazão de leitura
> A classe vem de `rotational` mais o prefixo do nome; NVMe e SATA SSD ficam juntos em WARM. Separá-los, e ordenar nós dentro de uma camada, exige benchmark de storage (análogo do `capability-benchmark` para compute).

> [!warning] Sem capacidade
> A classificação diz *quais* camadas o nó tem, não *quanto* espaço há em cada uma. Alocação (reserva de OS e recovery, swap, cache, checkpoint) não tem consumidor: nada no cluster coloca estado em disco por camada.

- Um dispositivo, uma classe: um nó com NVMe e HDD reporta `nvme,hdd`; dois NVMe não são distinguidos de um. Contagem e capacidade por dispositivo ficam para a fatia de alocação.
- `/sys/block` é do host em contêiner comum, como `/proc` nas métricas de nó. Em nó real, onde o worker roda no host, isso deixa de valer.
- O benchmark mede o dispositivo do diretório temporário, não um por classe; só escrita. A latência de escrita de 4 KiB com `fsync` (`storage_latency_ms`) já existe: num contêiner com disco rotativo mediu-se ~39 ms. NVMe e SATA SSD seguem na mesma camada WARM por nome; a latência é o dado que poderá separá-los.

## Próximos passos

- Benchmark de leitura (com O_DIRECT) e por dispositivo.
- Alocação por camada e política por classe (`f10-alocacao-por-camada`, `f10-politica-por-classe`), na fase [[fase-10-inteligencia-de-storage]].
