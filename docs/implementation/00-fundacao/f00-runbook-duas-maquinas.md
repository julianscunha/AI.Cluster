---
title: "Runbook de validação em duas máquinas"
type: operational
status: living
tags: [operational, two-machine-validation]
---

# Runbook de validação em duas máquinas

O benchmark no mesmo host prova a mecânica, mas não a afirmação com que a spec se importa:
que despachar uma unidade de trabalho entre máquinas custa menos do que o compute que ela
economiza. Em um único host os workers compartilham os mesmos núcleos, então o segundo
worker deixa os números *piores*. Só máquinas separadas respondem à pergunta real.

## O que isto prova

`T_total = T_compute + T_communication + T_serialization + T_memory + T_queue`

A linha de base de nó único (`ai-cluster benchmark --local`) é `T_compute` sem
transporte. A execução distribuída acrescenta todo o resto. Duas máquinas numa rede real
são o primeiro arranjo em que a comparação significa algo.

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

Confirme que B se registrou, a partir da máquina A:

```sh
cargo run -p ai-cluster-cli --release -- nodes
```

Rode a comparação — uma vez só com B, depois repita com um segundo worker em A para
ver se adicionar uma máquina supera adicionar núcleos:

```sh
cargo run -p ai-cluster-cli --release -- benchmark --size 256 --repetitions 8
```

## O que registrar

| execução | escopo | compute médio | transferência média | total |
|---|---|---|---|---|
| base | `--local` em A | | n/a | |
| 1 worker remoto | controller em A + worker B | | | |
| 2 workers | controller em A + workers A+B | | | |

A execução distribuída só ganha quando `compute médio economizado pela máquina extra`
supera a `transferência média`. Se a transferência fica estável enquanto o compute cai, a
distribuição compensa. Se a transferência cresce com o compute, a rede é o gargalo.

## Teto conhecido

O controller marca dispatch e conclusão em milissegundos, então o tempo de transferência tem
resolução de ~1ms (`work.rs`). Mantenha a carga na casa das centenas de milissegundos por
unidade, ou o ruído de medição domina. Aperte para timestamps em microssegundos antes de
fazer benchmark de unidades sub-milissegundo.
