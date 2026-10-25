---
title: "F00 — Runbook de validação em duas máquinas"
aliases: ["phase-0/two-machine-validation", "Duas máquinas"]
tags: [implementacao/fase/00, implementacao/fatia, implementacao/estado/parcial]
type: implementacao/fatia
status: parcial
superado: false
fase: "[[fase-00-fundacao]]"
codigos-antigos: ["phase-0/two-machine-validation", "roadmap-0"]
sistema: ["[[modelo-de-desempenho]]", "[[camada-de-transporte]]"]
evidencia: ["code/tests/scaling-sweep.sh"]
---
# F00 — Runbook de validação em duas máquinas

- [ ] Superado

## Objetivo

Obter a única medição que sustenta a afirmação de distribuição: que despachar uma Work Unit entre máquinas custa menos que o compute que ela economiza. Em um único host os workers dividem os mesmos núcleos, então o segundo worker piora os números. Só máquinas separadas respondem à pergunta real.

## Decomposição medida

`T_total = T_compute + T_communication + T_serialization + T_memory + T_queue`

A linha de base de nó único (`ai-cluster benchmark --local`) é `T_compute` sem transporte. A execução distribuída acrescenta o resto.

## O que foi feito

Runbook pronto e executável com a CLI atual. A execução física ainda não foi registrada. A mecânica está provada em host único e em dois containers ([[f00-testes-iniciais]]).

## Pré-requisitos

- Duas máquinas Linux (os "nós"), mesma arquitetura, alcançáveis pela rede.
- Toolchain Rust, ou a imagem `aic-rust:1` usada no CI.
- O controller faz bind em `0.0.0.0:50051`; o worker precisa alcançar essa porta.

## Passos

Na **máquina A** (controller):

```sh
cargo run -p ai-cluster-controller --release
```

Na **máquina B** (worker), apontando para o endereço de A:

```sh
AI_CLUSTER_NODE_ID=node-b \
AI_CLUSTER_CONTROLLER=http://<ip-da-maquina-a>:50051 \
cargo run -p ai-cluster-worker --release
```

Confirmar que B se registrou, a partir da máquina A:

```sh
cargo run -p ai-cluster-cli --release -- nodes
```

Rodar a comparação: uma vez só com B, depois repetir com um segundo worker em A para ver se adicionar uma máquina supera adicionar núcleos:

```sh
cargo run -p ai-cluster-cli --release -- benchmark --size 256 --repetitions 8
```

## Registro esperado

| execução | escopo | compute médio | transferência média | total |
|---|---|---|---|---|
| base | `--local` em A | | n/a | |
| 1 worker remoto | controller em A + worker B | | | |
| 2 workers | controller em A + workers A+B | | | |

A distribuição só ganha quando o `compute médio economizado pela máquina extra` supera a `transferência média`. Se a transferência fica estável enquanto o compute cai, a distribuição compensa. Se a transferência cresce com o compute, a rede é o gargalo.

> [!warning] Host único não prova distribuição
> Os números de host único e de dois containers são evidência de mecânica ([[f00-testes-iniciais]]). Nenhum ganho de distribuição deve ser alegado a partir deles.

## Limites

> [!warning] Teto de resolução
> O controller marca dispatch e conclusão em milissegundos, então o tempo de transferência tem resolução de ~1 ms (`work.rs`). Manter a carga na casa das centenas de milissegundos por unidade, ou o ruído domina. Apertar para timestamps em microssegundos antes de medir unidades sub-milissegundo.

## Próximos passos

- Executar nas duas máquinas físicas e preencher a tabela de registro esperado.
- Repetir com dois workers na máquina A para comparar com adicionar máquina.
- Trocar timestamps para microssegundos se a carga cair abaixo de 1 ms por unidade.
