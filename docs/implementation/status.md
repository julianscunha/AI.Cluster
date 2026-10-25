---
title: "Status consolidado"
aliases: ["Status da arquitetura", "STATUS.md", "Status"]
tags: [implementacao/transversal, implementacao/estado/em-andamento]
type: implementacao/transversal
status: em-andamento
---

# Status consolidado

Legenda: `[x]` implementado e validado · `[~]` parcial · `[ ]` não iniciado.

Derivado de `code/` (verdade do que roda), do [[implementation]] (tabela de fases), do plano mestre antigo e do roadmap antigo (ambos já divididos nas notas de fase). Atualizado em 2026-10-25. Os gates de cada fase estão em [[implementation]] e as provas por script em [[evidencias-e2e]].

## Tabela de fases

| Fase | Nome | Status | Gate (critério de saída) | Ressalva principal |
|---|---|---|---|---|
| [[fase-00-fundacao\|F00]] | Fundação | `[~]` | dois nós trocam uma Work Unit | provado só com containers no mesmo host |
| [[fase-01-nucleo-do-cluster\|F01]] | Núcleo do cluster | `[x]` | 2–4 máquinas sem operação manual | validado com containers; máquinas físicas pendentes |
| [[fase-02-capability-e-saude\|F02]] | Capability e saúde | `[~]` | perfil reproduzível | faltam memória sob carga, aceleradores e saúde de storage |
| [[fase-03-execucao-distribuida\|F03]] | Execução distribuída | `[x]` | perder um worker não corrompe o DAG | — |
| [[fase-04-escalonamento\|F04]] | Escalonamento | `[x]` | melhor que round-robin | G4 `[~]` (sem filas de recurso); G1 trust `[~]` (quarentena automática `[x]`; níveis por enrollment `[ ]`) |
| [[fase-05-observabilidade\|F05]] | Observabilidade | `[x]` | toda requisição com trace e métricas | sem TTFT, ITL, tokens/s, KV cache |
| [[fase-06-estado-e-recuperacao\|F06]] | Estado e recuperação | `[x]` | recupera de falha planejada e abrupta | estado por chave não replicado (vai para F12) |
| [[fase-07-seguranca-base\|F07]] | Segurança base | `[x]` | nó desconhecido não recebe trabalho (NFR-011) | mTLS e attestation vão para F19 |
| [[fase-08-inferencia\|F08]] | Inferência | `[~]` | logits idênticos local × fatiado; 503 sem pendurar | gateway concentra o modelo; sem Request Manager |
| [[fase-09-transporte\|F09]] | Transporte | `[~]` | mesma Work Unit em transportes distintos | só gRPC/TCP; trait `Transport` é esqueleto |
| [[fase-10-inteligencia-de-storage\|F10]] | Inteligência de storage | `[~]` | políticas HDD × NVMe mensuráveis | classes detectadas; sem políticas dinâmicas |
| [[fase-11-request-manager\|F11]] | Request Manager | `[ ]` | quotas e fila sob carga | — |
| [[fase-12-model-runtime\|F12]] | Model Runtime distribuído | `[ ]` | overhead medido; M4 | N4 (sharding) e N6 (CPU) adiantados em F08 |
| [[fase-13-graph-engine\|F13]] | Graph engine | `[ ]` | só adotar com ganho medido | — |
| [[fase-14-edge-optimizer\|F14]] | Edge optimizer | `[ ]` | ganho > custo; M5 | — |
| [[fase-15-ai-cluster-os\|F15]] | AI Cluster OS e provisionamento | `[ ]` | máquina limpa vira worker; M2 | — |
| [[fase-16-aceleradores\|F16]] | Aceleradores | `[ ]` | M6 | — |
| [[fase-17-runtime-adaptativo\|F17]] | Runtime adaptativo | `[ ]` | melhora estatística | — |
| [[fase-18-worker-movel\|F18]] | Worker móvel | `[ ]` | M7 | — |
| [[fase-19-hardening-de-producao\|F19]] | Hardening de produção | `[ ]` | soak e failure injection | — |

Marcos (M0–M7) e seus critérios: [[marcos]].

## Slices por fase

### F00 — Fundação `[~]`

- `[x]` [[f00-estrutura-do-repositorio]] — estrutura, builds, CI, unit test harness, tipos comuns (A1–A3).
- `[~]` [[f00-mvp]] — dois nós trocam Work Unit e a varredura roda (§20 executado); a prova entre máquinas físicas não.
- `[ ]` [[f00-runbook-duas-maquinas]] — runbook escrito; execução em duas máquinas físicas não feita.
- `[~]` [[f00-testes-iniciais]] — provável superado pela suíte atual ([[estrategia-de-testes]]).

### F01 — Núcleo do cluster `[x]`

- `[x]` [[f01-entrada-e-saida-de-nos]] — registro, heartbeat, saída planejada, CLI `status`/`nodes`/`node show`/`events`. Validado com containers.

### F02 — Capability e saúde `[~]`

- `[~]` [[f02-benchmark-de-capability]] — compute medido no kernel do worker; memória sob carga, storage e aceleradores não medidos.
- `[x]` [[f02-capability-corrente]] — baseline + current + confiança; mediana e desvio do registro.
- `[x]` [[f02-estabilidade-do-benchmark]] — distribuição do registro calibra a previsão.
- `[x]` [[f02-modelo-de-saude]] — liveness, pressão de memória, swap e térmica.
- `[~]` [[f02-descoberta-de-rede]] — velocidade nominal do link `[x]`; sonda de upload no registro e na atual `[~]`; sem latência nem perda.

### F03 — Execução distribuída `[x]`

- `[x]` [[f03-grafo-de-dependencias]] — DAG com branches paralelas e ordenação (diamante).
- `[x]` [[f03-tolerancia-a-falhas]] — worker morto: unidades reenfileiradas, grafo drena.
- `[x]` [[f03-varredura-compute-vs-transferencia]] — primeiro experimento (§20). Cruzamento acima de 256 (teto de mensagem gRPC).

### F04 — Escalonamento `[x]`

- `[x]` [[f04-placement]] — filtro por capability e memória, custo com histerese, previsão de tempo. Locality não modelada; térmico entra pelo score de saúde.
- `[~]` [[f04-scheduler-local]] — concede cores `[x]`; prioridade por unidade na fila do controller `[x]`; filas de recurso no worker e GPU/NPU `[ ]`.
- `[x]` [[f04-reservas]] — `Reserve`/`ListReservations`/`ReleaseReservation`. Não persiste após restart; retém o nó inteiro.

### F05 — Observabilidade `[x]`

- `[x]` [[f05-eventos]] — ring buffer de eventos (`ai-cluster events`); não é event bus.
- `[x]` [[f05-metricas]] — roll-up, p50/p95, exportador `GET /metrics`. Rede e storage score fora do roll-up.
- `[x]` [[f05-traces]] — `request_id` liga requisição às unidades. Não persiste além de restart.
- `[x]` [[f05-dashboard]] — `ai-cluster dashboard [--watch N]`. Campos de LLM ausentes por falta de fonte.
- `[x]` [[f05-previsto-vs-real]] — previsão de tempo por unidade e no roll-up; erro caiu de ~32× para poucos por cento.

### F06 — Estado e recuperação `[x]`

- `[x]` [[f06-persistencia-e-checkpoint]] — snapshot da fila e dos donos em voo; checkpoint podado. Ainda é snapshot, não journal. Registry e eventos não sobrevivem a restart.
- `[x]` [[f06-recuperacao]] — restart do controller com unidade em voo; cópia do estado num nó (promoção de réplica). Estado por chave (KV cache, execution state, model state) não existe.

### F07 — Segurança base `[x]`

- `[x]` [[f07-token-de-join]] — token exigido em toda RPC; controller não sobe sem token.
- `[x]` [[f07-aprovacao-de-no]] — nó pendente não recebe trabalho até `approve`; aprovação persiste; `revoke` derruba o worker.
- `[x]` [[f07-operador-e-credencial-por-no]] — token de operador separado para RPCs administrativas (S3a) e credencial por nó emitida no registro (S3b). Sem script E2E dedicado.
- `[x]` [[f07-tls]] — TLS no canal gRPC com CA; `AI_CLUSTER_INSECURE` explícito. mTLS e attestation vão para F19.

### F08 — Inferência `[~]`

- `[x]` [[f08-gateway-e-api]] — gateway HTTP com chave de API, `/v1/models`, `/v1/embeddings` (modelo sintético sobre matmul nos workers). Gateway roda o modelo; nós escolhidos por variável de ambiente; sem Request Manager.
- `[x]` [[f08-engine-local]] — forward do GPT-2 small e da SmolLM2-135M em CPU com paridade contra `transformers`; geração gulosa com KV cache. (J2, J4; J4 confirmado pelo commit `8c58489`.)
- `[x]` [[f08-chat-completions]] — `/v1/chat/completions` com streaming SSE, cancelamento ao desconectar, template ChatML. Roda no gateway, fora do cluster.
- `[x]` [[f08-canal-residente-de-fatias]] — projeções em workers (GPT-2 por colunas, SmolLM2 por linhas), reatribuição de faixas se um worker cai, 503 sem pendurar, papel de inferência separado. Só as projeções são distribuídas.

### F09 — Transporte `[~]`

Sem slices próprios no manifesto. Estado em [[fase-09-transporte]].

- `[x]` E2 — gRPC sobre TCP.
- `[~]` E1 — `crates/transport` tem só `enum TransportKind` e `trait Transport { fn kind() }`; o cliente gRPC é usado direto.
- `[~]` E5 — sonda de upload do registro (`ProbeLink`); sem métricas adaptativas de latência ou perda.
- `[ ]` E3 (memória compartilhada) e E4 (USB, que vai para F18).

### F10 — Inteligência de storage `[~]`

- `[~]` [[f10-classes-de-armazenamento]] — classes HOT/WARM/COLD por nó lidas de `/sys/block` (NVMe/SSD → WARM, HDD → COLD) `[x]`; escrita sequencial com fsync `[~]`; latência, SMART, alocação dinâmica e políticas `[ ]`.

### F11 a F19 `[ ]`

Não iniciados. Ver as notas de fase. Pontos de partida já existentes:

- F12: projeções sharded e CPU forward estão em F08 (N4 e N6 parciais).
- F15: nada de OS; `code/os/README.md` define só a fronteira.
- F16: não há caminho de execução de acelerador; C1 `accelerators` `[ ]`.

## Itens corrigidos em relação às fontes antigas

Estes itens estavam defasados nas fontes antigas e foram corrigidos aqui:

| Fonte | Item | Situação real |
|---|---|---|
| EP G1 | trust "sem identidade por nó, sem TLS" | S3b (credencial por nó) e S4 (TLS) existem. F07 `[x]`. |
| EP K1 | detecção HDD/SSD/NVMe `[ ]` | `/sys/block` classifica por nó (I2, `phase-i-storage.sh`). |
| EP H4 | "TUI" `[x]` sem detalhe | `ai-cluster dashboard`, sem TTFT/ITL/tokens/s. |
| STATUS | "TUI não iniciada" | existe (`dashboard`, F05). |
| STATUS | "storage com detecção HDD/SSD/NVMe `[ ]`" | classes detectadas `[x]`; políticas `[ ]`. |
| STATUS | "performance model expected vs actual `[ ]`" | `[x]` (F05, previsto vs real). |
| STATUS | "linguagem Controller/Worker: aberto" | Rust (`code/crates/*`, ADR-008). |
| STATUS | "gRPC + Protobuf: proposto"; "mTLS: proposto" | gRPC + Protobuf em uso; TLS implementado; mTLS fica em F19. |
| EP B4 / roadmap Fase 1 | "2–4 máquinas" | só containers; ver [[f00-runbook-duas-maquinas]]. |

## Marcas do STATUS.md (decisões de arquitetura)

Estas marcas vão para `[[estado-das-decisoes]]` (decisões) e para cá (marcas de implementação).

| Item | Marca | Fase |
|---|---|---|
| Linux em todos os nodes | `[x]` | F00 |
| Mesmo AI Cluster OS lógico com hardware distinto | `[ ]` | F15 |
| Reimage após ingresso | `[ ]` | F15 |
| PCs, servidores, SBCs e celulares | `[ ]` | F15–F18 |
| Hardware heterogêneo | `[~]` | F02, F04 |
| Hot-plug / hot-remove | `[~]` | F01, F06 |
| Capability scoring multidimensional | `[~]` | F02 |
| Health / Capacity / Trust separados | `[~]` | F02, F04 |
| Controller + Worker Runtime | `[x]` | F01 |
| Work Unit | `[x]` | F03 |
| Execution Graph + Placement Plan | `[x]` | F03, F04 |
| Pipeline / Tensor / Hybrid parallelism | `[ ]` | F12 |
| Edge optimization antes da inferência pesada | `[ ]` | F14 |
| Graph engine | `[ ]` | F13 |
| Storage intelligence (HDD/SSD/NVMe) | `[~]` | F10 |
| SSD/NVMe para cache/swap | `[ ]` | F10 |
| Telemetria detalhada e TUI via shell | `[x]` | F05 |
| Performance Model expected vs actual | `[x]` | F05 |
| Application Plane via Inference API | `[~]` | F08 |
| Request Manager separado do Scheduler | `[ ]` | F11 |
| Inference Engine separado do Scheduler | `[~]` | F08 |

Proposto, sem marca de implementação: base Linux específica (`[ ]`); PXE/iPXE (`[ ]`, F15); PostgreSQL + time-series + object storage + graph projection (`[ ]`, F13, F10). Experimental e aberto permanecem como estão em `[[estado-das-decisoes]]`.
