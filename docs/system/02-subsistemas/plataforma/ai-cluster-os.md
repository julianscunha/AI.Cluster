---
title: "AI Cluster OS"
aliases: ["docs/02", "02 — AI Cluster OS"]
tags: [sistema/subsistema/plataforma]
type: sistema/subsistema
status: proposto
implementacao: nao-iniciado
fases: ["[[fase-15-ai-cluster-os]]", "[[fase-16-aceleradores]]", "[[fase-18-worker-movel]]"]
---

# AI Cluster OS

Sistema operacional mínimo, versionado e orientado a compute, igual em todos os nós. Ele existe
para que um nó seja só um worker: sem ambiente de desktop, sem serviços de consumo, com a
identidade de provisionamento e o runtime do worker já instalados, e com atualização que se
reverte. Não é uma distribuição nova: é uma imagem de uma distribuição Linux existente, montada
e assinada pelo projeto.

## Requisitos

- Multi-arquitetura: x86_64 e ARM64.
- Boot e recuperação: a máquina volta a um estado conhecido sem intervenção manual.
- Rede primeiro: o nó sobe, obtém endereço e alcança o controller sem configuração local.
- Runtime do worker instalado e supervisionado.
- Monitoramento de hardware e inteligência de armazenamento (ver [[storage-intelligence]]).
- Identidade segura: a credencial e a identidade do nó vêm da imagem e do provisionamento, não de
  um arquivo editado à mão (ver [[seguranca]]).
- Atualização A/B, com rollback.
- Pouco uso de recurso em segundo plano.

## Base do sistema

Escolha proposta: uma distribuição Linux enxuta e imutável, existente, em vez de uma distribuição
mantida do zero. Ver [[adr-001-base-linux]] e [[adr-002-os-imutavel]].

A escolha final considera suporte do kernel, ARM64, boot seguro, ecossistema de drivers,
construção da imagem, recuperação e ciclo de vida. Esses critérios estão em aberto.

## Atualização imutável

- Imagens versionadas. Um update troca a imagem inteira, não arquivos isolados.
- Duas partições ou slots (A/B). Um update grava no slot inativo, reinicia e só então confirma.
  Se o slot novo não passar pela validação de boot, volta ao anterior.
- O estado do nó (fila local, cache, checkpoints) fica em volume separado da imagem, para que
  trocar a imagem não apague o estado.

## Layout do runtime

```text
/usr/lib/ai-cluster/
  core/
  worker/
  hardware/
  telemetry/
  recovery/

/etc/ai-cluster/
  node.toml
  identity/

/var/lib/ai-cluster/
  cache/
  state/
  models/
  checkpoints/
```

Caminhos propostos; detalhes finais podem mudar.

## Minimização de recursos

Desabilitar o que não serve ao compute, quando o hardware permitir: interface gráfica, impressão,
serviços de consumo, indexadores e qualquer carga que não seja do cluster.

## Tuning de kernel

Candidatos: governador de CPU, política de memória, huge pages, afinidade de IRQ, buffers de rede,
escalonador de I/O, awareness de NUMA quando houver, política térmica. Cada ajuste é medido antes de
virar padrão global.

## Papel na provisão

O OS é o alvo do reimage. Um dispositivo só vira worker depois de receber a imagem do AI Cluster OS
e passar pela atestação pós-boot. O fluxo está em [[provisionamento]].

> [!info] O que já existe
> - Nada de imagem de SO. `code/os/README.md` declara que o trabalho da imagem começa na fase de
>   provisionamento e que a Fase 0 só define a fronteira do runtime.
> - Estados de provisionamento existem no enum de nó (`PROVISIONING` em `code/crates/common/src/lib.rs`),
>   mas nenhum caminho do código os atinge.
> - O worker roda como processo comum, configurado por variáveis de ambiente
>   (`AI_CLUSTER_CONTROLLER`, `AI_CLUSTER_NODE_ID`, `AI_CLUSTER_NODE_CREDENTIAL_FILE`), não como serviço
>   de um OS próprio: ver [[worker-runtime]].

> [!todo] Tudo do que este documento descreve está pendente: escolha de base, build da imagem,
> assinatura, slots A/B, boot e recuperação, multi-arquitetura, tuning medido e serviços
> desabilitados. Ver [[fase-15-ai-cluster-os]]; ARM64 e GPU em [[fase-16-aceleradores]].

> [!warning] Desvio do alvo: no MVP o worker roda em contêiner ou processo sobre um Linux qualquer,
> sem imagem própria, sem A/B e sem garantia de que o SO do nó é o esperado. Até a imagem existir, a
> atestação de SO ([[seguranca]]) não tem o que validar.
