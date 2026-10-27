---
title: "Estado das decisões"
aliases: ["STATUS", "Status da arquitetura", "Estado da arquitetura", "Decisões vigentes"]
tags: [sistema/arquitetura]
type: sistema/arquitetura
---

# Estado das decisões

Voltar para [[arquitetura]]. Esta página separa as decisões vigentes, as propostas, as experimentais e as questões de desenho ainda abertas. O registro de cada decisão, com contexto, alternativas e consequências, está em [[decisoes]].

## Vigentes

| Decisão | ADR |
|---|---|
| Linux em todos os nodes. | [[arquitetura]] §7, [[adr-001-base-linux]] |
| Rust como linguagem principal do Controller e do Worker. | [[adr-008-stack-inicial]] |
| gRPC + Protocol Buffers como RPC e schema do control plane. | [[adr-008-stack-inicial]], [[protocolo-controller-worker]] |
| Controller separado do Worker Runtime. | [[adr-003-controller-worker]] |
| Work Unit como unidade de execução. | [[work-unit]] |
| Hardware tratado por capability, não por modelo de máquina. | [[principios]] P-001, [[capability-engine]] |
| Health, capacity e trust separados. | [[saude-e-confianca]], [[principios]] P-004 e P-005 |
| Execution Graph (WHAT), placement (WHERE) e Worker Runtime (HOW) separados. | [[principios]] P-003 |
| O nó pede trabalho e o controller decide por custo, com histerese, prioridade e quarentena por evidência. | [[adr-011-escalonamento-por-custo]] |
| Hot-plug por state objects, checkpoints e replanejamento, sem migração de memória viva. | [[adr-006-hotplug]] |
| RAM e VRAM como HOT, SSD e NVMe como WARM, HDD como COLD. | [[adr-005-camadas-de-storage]] |
| A Inference API é a única fronteira para cargas externas. | [[adr-007-fronteira-inference-api]] |
| Pesos residentes nos workers, fatiados por faixas, com canal bidirecional pelo controller. | [[adr-009-canal-residente-de-fatias]] |
| Papéis de credencial separados (join, operador, inferência, credencial por nó, chave de API), com falha fechada. | [[adr-010-papeis-de-credencial]] |
| O gateway mede TTFT, ITL, tokens/s e KV cache de cada geração. | [[adr-012-metricas-de-inferencia-no-gateway]] |
| Estado do controller por snapshot do conjunto vivo, posse preguiçosa e cópia num nó. | [[adr-013-estado-do-controller-e-recuperacao]] |
| O transporte carrega bytes em frames; o cluster segue em gRPC. | [[adr-014-abstracao-de-transporte]] |
| Request Manager dentro do gateway, enquanto houver um só gateway. | [[adr-015-request-manager-no-gateway]] |
| mTLS opcional no plano de controle, ligado por CA de clientes. | [[adr-016-mtls-opcional-no-plano-de-controle]] |
| Instalação A/B imutável é o caminho oficial; boot de rede só instala e recupera. | [[adr-017-instalacao-a-b-e-boot-de-rede]] |
| Release assinada (Ed25519) conferida no nó, versão sempre crescente e medida do slot como attestation de software. | [[adr-018-release-assinada-e-medida-do-slot]] |

## Propostas

Decisões sugeridas que ainda não foram tomadas.

| Proposta | ADR |
|---|---|
| Base Linux específica. | [[adr-001-base-linux]] |
| Imagem versionada e imutável com atualização A/B como padrão do OS do nó. | [[adr-002-os-imutavel]] |
| Grafo como projeção, não como fonte única de verdade. | [[adr-004-grafo-como-projecao]] |
| PostgreSQL, time-series, object storage e projeção em grafo para metadados e métricas. | [[state-manager]], [[arquitetura]] §17 |
| PXE/iPXE para provisionamento de PCs e servidores. | [[provisionamento]] |

## Experimentais

Precisam de benchmark ou prova de conceito antes de virar decisão.

| Tema | Nota |
|---|---|
| Mobile boot/reimage universal. | [[provisionamento]] |
| Backends de NPU. | [[ai-cluster-os]] |
| RDMA. | [[camada-de-transporte]] |
| Zero-copy cross-device. | [[camada-de-transporte]] |
| Limiares de execução especulativa. | [[execution-planner]] |
| Planejador aprendido. | [[execution-planner]] |

## Questões em aberto

| Questão | Nota |
|---|---|
| Base Linux exata. | [[adr-001-base-linux]] |
| Formato interno de tensor e shard. | [[model-manager]] |
| Tecnologia de graph database e de projeção. | [[graph-engine]] |
| Tecnologia de time-series. | [[observabilidade]] |
| Modelo de armazenamento distribuído do estado. | [[state-manager]], [[storage-intelligence]] |
| Quando adotar pipeline ou paralelismo híbrido (modelo que não cabe em um nó). | [[inference-engine]] |
| Rebalanceamento de fatias quando um nó entra no cluster. | [[canal-de-fatias]] |
| Formato do worker móvel. | [[provisionamento]] |
| Estratégia final de kernel optimization. | [[worker-runtime]] |
| Imagem para ARM64 e x86_64 (instalador e bootloader próprios). | [[adr-017-instalacao-a-b-e-boot-de-rede]] |
| Assinatura do boot, dm-verity e TPM. | [[adr-018-release-assinada-e-medida-do-slot]] |
| Revogação de chave de release sem imagem nova. | [[adr-018-release-assinada-e-medida-do-slot]] |
| Ligação do certificado de cliente ao `node_id`. | [[adr-016-mtls-opcional-no-plano-de-controle]] |
