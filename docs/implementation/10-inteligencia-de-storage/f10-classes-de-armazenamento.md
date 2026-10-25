---
title: "Classificação de armazenamento por nó"
type: implementation
status: implemented
tags: [implementation, storage, state-manager, phase-i]
---

# Classificação de armazenamento por nó

O nó reportava a RAM e nada sobre disco. O modelo de camadas do
`docs/13-state-manager.md` — HOT, WARM, COLD — existia no papel desde o começo, mas nada
no cluster sabia em qual camada um nó podia guardar estado. Esta é a fatia I2: cada nó diz
de que classes de armazenamento dispõe.

## O que mudou

- `CapabilityReport.storage_classes` no proto: as classes em bloco detectadas no nó, da
  mais quente para a mais fria. O campo existia como `storage_score` (um `double` nunca
  preenchido); um escalar não carrega *quais* camadas existem, então a classificação virou
  uma lista.
- `storage_class` / `storage_classes_from` / `storage_classes` no worker: leem
  `/sys/block/*/queue/rotational` e traduzem em classe.
- `ai-cluster nodes` ganhou a coluna `ARMAZ`.

Sem dependência nova: `/sys/block` é um `read_dir` e um `read_to_string`, o mesmo que o
worker já faz com `/proc`.

## Semântica

O kernel responde `rotational`: 0 é estado sólido, 1 é disco giratório. Isso separa HDD de
SSD, mas **não** separa NVMe de SATA SSD — os dois respondem 0. O nome do dispositivo é a
única pista que sobra sem abrir o dispositivo, então `nvme` vem do prefixo do nome. É uma
heurística, não uma medida de latência: um NVMe e um SATA SSD ficam na mesma camada WARM
até existir um benchmark de storage (o análogo do C2 para compute).

Loop, ram e device-mapper ficam de fora. Existem em qualquer nó, não são armazenamento que
o cluster possa usar, e contá-los faria todo nó parecer ter disco.

A classificação é **por dispositivo**, não por camada. A tradução para as camadas do
`docs/13` é: HDD é COLD, SSD/NVMe são WARM. O HOT é RAM e já vinha em `memory_bytes` —
duplicá-lo aqui só criaria dois números para a mesma coisa.

Vazio é "não medido", nunca "sem armazenamento". Um nó que não expõe `/sys/block` reporta
uma lista vazia; a CLI imprime `-`. É a mesma disciplina das métricas de nó: imprimir zero
ali diria que o valor é zero, que é diferente de não existir.

`AI_CLUSTER_STORAGE_CLASSES` fixa as classes de um nó, como `AI_CLUSTER_CPU_THREADS` e
`AI_CLUSTER_MEMORY_BYTES` já fazem. Sem ela a resposta é do hardware do host, e um contêiner
comum vê `/sys/block` do host, não o dele — que é exatamente por que o E2E precisa de um
nó com classes fixadas para provar o caminho.

## Benchmark de escrita (Fase K2)

O worker escreve 16 MiB com `fsync` (melhor de 3) no diretório temporário e reporta a taxa em
`storage_score`, relativa a 500 MB/s (um SATA SSD): 1.0 é isso, ~0.2 um HDD, ~6 um NVMe. O
`fsync` é o que faz o número ser do disco; sem ele o page cache responde. O detalhe do nó
mostra `armaz_score=`; `-` é "não medido". Evidência: `phase-c-network.sh` A) `armaz_score=0.06`
(overlay do Docker Desktop, que não representa o disco do host).

`ai-cluster nodes` ganhou a coluna `ARMAZ_SCORE` (`-` é não medido), depois de `ARMAZ`.

Tetos: mede o dispositivo do diretório temporário, não um por classe de `storage_classes`;
só escrita, sem latência; o único consumidor é a coluna da CLI: nenhuma unidade de trabalho toca disco (os operandos passam pelo controller e a réplica vai a todo nó atrasado), então um termo de placement seria sempre zero ou inventado, e
sem pin de determinismo. A heurística de nome (`nvme`) segue valendo até o score ser lido.

## Evidência

Três nós, um com classes fixadas, um sem override e um com override vazio:

```
NÓ                   ESTADO     ARQ        THREADS    RAM_GB COMPUTE    SAÚDE    CPU_%  TEMP_C  SWAP_% ARMAZ
node-blank           READY      x86_64           4      3.77    1.19     0.74     2.10    0.00    1.14 -
node-fixed           READY      x86_64           4      3.77    1.16     0.74     1.45    0.00    1.14 nvme,ssd
node-real            READY      x86_64           4      3.77    1.17     0.74     1.40    0.00    1.14 hdd
```

`node-real` é a detecção real: no contêiner o host apresenta os discos como `rotational=1`,
então a resposta honesta é `hdd`. O E2E é `bash code/tests/phase-i-storage.sh`; os testes de
unidade são `classifies_block_devices_by_rotation_and_name` e
`storage_classes_are_distinct_and_hottest_first` em `worker/src/main.rs`.

## Teto conhecido

**Sem medida de latência ou vazão.** A classe vem de `rotational` mais o prefixo do nome;
NVMe e SATA SSD ficam juntos em WARM. Separar os dois — e ordenar nós dentro de uma camada —
precisa de um benchmark de storage, o análogo do `capability-benchmark.md` para compute.

**Sem capacidade.** A classificação diz *quais* camadas o nó tem, não *quanto* espaço há em
cada uma. Alocação — reserva de OS/recovery, swap, cache, checkpoint — é o próximo degrau do
`docs/18-storage-intelligence.md` e não tem consumidor ainda: nada no cluster coloca estado
em disco por camada.

**Um dispositivo, uma classe.** Um nó com NVMe e HDD reporta `nvme,hdd`; um nó com dois
NVMe não é distinguido de um com um. Contagem e capacidade por dispositivo ficam para a
fatia de alocação.

**`/sys/block` é do host em contêiner comum.** Como `/proc` nas métricas de nó, a detecção
enxerga os discos do host inteiro, não os do contêiner. Em nó de verdade, onde o worker roda
no host, isso deixa de ser uma ressalva.
