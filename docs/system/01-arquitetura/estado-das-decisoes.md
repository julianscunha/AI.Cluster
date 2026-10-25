---
title: "Estado das decisões"
aliases: ["STATUS", "Status da arquitetura", "Estado da arquitetura"]
tags: [sistema/arquitetura]
type: sistema/arquitetura
status: vivo
implementacao: parcial
---

# Estado das decisões

Voltar para [[arquitetura]]. Esta nota separa **o estado da decisão** (decidido, proposto, experimental, aberto) do **estado da implementação** (marcas abaixo). O estado de implementação por fase e por slice está em [[status]].

Legenda de implementação: `[x]` implementado e validado · `[~]` parcial · `[ ]` não iniciado.

## Decidido

| Decisão | Impl. | Situação atual | Onde |
|---|---|---|---|
| Linux em todos os nodes. | [x] | Controller e workers rodam em Linux. | [[arquitetura]] §7 |
| Rust como linguagem principal do Controller e do Worker. | [x] | Workspace Rust (edition 2024) em `code/`. | [[adr-008-stack-inicial]] |
| gRPC + Protocol Buffers como RPC e schema do control plane. | [x] | `code/proto/cluster.proto`, tonic 0.14 e prost 0.14. | [[adr-008-stack-inicial]], [[protocolo-controller-worker]] |
| Controller + Worker Runtime. | [x] | Separação implementada. | [[adr-003-controller-worker]] |
| Work Unit como unidade de execução. | [x] | Unidade de trabalho com DAG e retries. | [[work-unit]] |
| Mesmo AI Cluster OS lógico com hardware distinto. | [ ] | Ainda não há imagem própria. | [[ai-cluster-os]], [[adr-002-os-imutavel]] |
| Reimage do sistema original após ingresso; não executar apenas um appliance sobre o Linux antigo. | [ ] | Reimage não implementado. | [[provisionamento]], [[adr-002-os-imutavel]] |
| Suporte conceitual a PCs, servidores, SBCs e celulares compatíveis. | [ ] | Não implementado. | [[provisionamento]], [[fase-18-worker-movel]] |
| Hardware heterogêneo. | [~] | Nós de tamanhos diferentes já são filtrados por capability. | [[capability-engine]], [[fase-02-capability-e-saude]] |
| Hot-plug / hot-remove. | [~] | Join/leave seguro validado; recuperação de estado em aberto. | [[hotplug-e-recuperacao]], [[adr-006-hotplug]] |
| Capability scoring multidimensional. | [~] | Threads, RAM e health são reportados; scoring multidimensional ainda não. | [[capability-engine]] |
| Health, Capacity e Trust separados. | [~] | Health e capacity são reportados; trust ainda parcial. | [[saude-e-confianca]], [[seguranca]] |
| Execution Graph + Placement Plan. | [~] | DAG com dependências e retries validado; custo de placement em `placement.rs`, sem locality nem fator térmico. | [[execution-graph]], [[execution-planner]], [[fase-04-escalonamento]] |
| Pipeline / Tensor / Hybrid parallelism. | [ ] | Não implementado. | [[fase-12-model-runtime]] |
| Edge optimization antes da inferência pesada. | [ ] | Não implementado. | [[edge-optimizer]], [[fase-14-edge-optimizer]] |
| Graph engine para relações e retrieval. | [ ] | Não implementado. | [[graph-engine]], [[adr-004-grafo-como-projecao]] |
| Storage intelligence com detecção HDD/SSD/NVMe. | [~] | Classe por dispositivo (HDD = COLD, SSD/NVMe = WARM) no worker; a fase segue parcial. | [[storage-intelligence]], [[adr-005-camadas-de-storage]] |
| SSD/NVMe aproveitados para cache/swap conforme política. | [ ] | Não implementado. | [[storage-intelligence]], [[fase-10-inteligencia-de-storage]] |
| Telemetria detalhada e TUI via shell. | [~] | Eventos e métricas prontos; o dashboard de texto existe no CLI (`--watch`); TUI full-screen com ratatui não iniciada. | [[observabilidade]], [[cli-tui]] |
| Performance Model expected vs actual. | [~] | Previsto vs real já existe no roll-up de métricas; sem calibração contínua do modelo. | [[modelo-de-desempenho]], [[f05-previsto-vs-real]] |
| Application Plane desacoplado via Inference API. | [~] | Gateway com chave de API, `/v1/models`, `/v1/embeddings` e `/v1/chat/completions`; o modelo roda no próprio gateway, fora do cluster. | [[inference-api]], [[adr-007-fronteira-inference-api]] |
| Request Manager separado do Scheduler. | [ ] | Não implementado. | [[request-manager]], [[fase-11-request-manager]] |
| Inference Engine separado do Planner/Scheduler. | [~] | Forward de GPT-2 small e SmolLM2-135M em CPU com paridade contra `transformers`; projeções em workers pelo canal residente de fatias. | [[inference-engine]], [[adr-009-canal-residente-de-fatias]] |

## Proposto

| Proposta | Situação atual | Onde |
|---|---|---|
| Base Linux específica. | Não escolhida. | [[adr-001-base-linux]] |
| PXE/iPXE para provisionamento. | Não implementado. | [[provisionamento]] |
| PostgreSQL + time-series + object storage + projeção em grafo. | Hoje: snapshot de arquivo e roll-up em memória. | [[state-manager]], [[arquitetura]] §17 |
| mTLS. | TLS implementado ([[fase-07-seguranca-base]]); mTLS e attestation na fase de hardening. | [[seguranca]], [[fase-19-hardening-de-producao]] |

## Experimental

Precisam de benchmark ou prova de conceito antes de virar decisão.

| Tema | Onde |
|---|---|
| Mobile boot/reimage universal. | [[fase-18-worker-movel]] |
| NPU backends. | [[fase-16-aceleradores]] |
| RDMA. | [[camada-de-transporte]] |
| Zero-copy cross-device. | [[camada-de-transporte]] |
| Speculative execution thresholds. | [[fase-17-runtime-adaptativo]] |
| Learned planner. | [[fase-17-runtime-adaptativo]], [[execution-planner]] |

## Aberto

| Questão | Onde |
|---|---|
| Base Linux exata. | [[adr-001-base-linux]] |
| Formato interno de tensor e shard. | [[model-manager]], [[fase-12-model-runtime]] |
| Tecnologia de graph database / projeção. | [[graph-engine]] |
| Tecnologia de time-series. | [[observabilidade]] |
| Modelo de armazenamento distribuído. | [[state-manager]], [[storage-intelligence]] |
| Formato do worker móvel. | [[fase-18-worker-movel]] |
| Estratégia final de kernel optimization. | [[fase-16-aceleradores]] |

> [!note] Correções em relação ao STATUS anterior
> - A linguagem principal do Controller e do Worker saiu de *Aberto* e virou *Decidido* (Rust, [[adr-008-stack-inicial]]).
> - A TUI deixou de ser "não iniciada": existe o dashboard de texto do CLI. Só a TUI full-screen segue pendente.
> - Storage deixou de ser "não iniciado": a classificação por dispositivo existe no worker. Alocação dinâmica e aproveitamento de SSD/NVMe seguem pendentes.
> - gRPC + Protobuf saiu de *Proposto* e virou *Decidido* (ADR-008), já em uso.
> - Previsto vs real deixou de ser "não iniciado": o roll-up de métricas já compara previsto e real.
