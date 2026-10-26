---
title: "F00 — Fundação"
aliases: ["phase-0", "Fase 0", "Fase 0 — Fundação", "roadmap 0", "EP A", "Fase 0 — Arquitetura e harness de benchmark"]
tags: [implementacao/fase/00, implementacao/estado/parcial]
type: implementacao/fase
status: parcial
---
# F00 — Fundação

## Objetivo

Estabelecer o repositório, os tipos comuns e o protocolo sobre os quais o cluster é construído, e provar que dois nós trocam uma Work Unit e medem compute contra transferência. Não há modelo de linguagem nesta fase: o gate é o substrato distribuído.

## Depende de

Nada. É a primeira fase da cadeia (ver [[implementation]]).

## Entrega

- Workspace Rust reproduzível, CI e harness de testes ([[f00-estrutura-do-repositorio]]).
- Tipos comuns (IDs, timestamps, enums de status, modelo de erro, capability, serialização) e contrato gRPC em `code/proto/cluster.proto`.
- Esqueleto do protocolo: registro, heartbeat, capabilities e health.
- Esqueleto do node agent e harness de benchmark (`ai-cluster benchmark`).
- Esquema de telemetria: eventos e métricas agregadas ([[f05-eventos]], [[f05-metricas]]).
- Workload de matriz local (`ai-cluster benchmark --local`).
- Primeiro experimento de compute contra comunicação ([[f00-mvp]]).

## Checklist

Derivado de EP §2 (A1–A3), roadmap Fase 0 e M0.

- [x] **A1 Repositório**: builds reproduzíveis, versionamento, CI e unit test harness.
- [x] **A2 Tipos comuns**: controller e worker compartilham schemas sem duplicação.
- [x] **A3 Protocolo**: registro, heartbeat, capabilities e health; dois nós mantêm sessão e reportam estado.
- [x] Harness de benchmark e workload de matriz local.
- [x] Esquema de telemetria (eventos e métricas).
- [x] Esqueleto do protocolo e do node agent.
- [x] Critério de saída: dois nós Linux trocam uma Work Unit e comparam compute contra transferência. Provado entre o PC (x86) e um Orange Pi (ARM) na LAN, com paridade numérica ([[f00-validacao-em-hardware-real]]). O nó x86 é um container num Docker Desktop.
- [x] M0 (aceite) — dois nós Linux, descobertos pelo controller, um envia operação de matriz ao outro, telemetria registrada. Ressalva: validado com dois containers; máquinas físicas pendentes ([[marcos]]).
- [x] Primeiro experimento de EP §20 executado (1/2/3 nós × 32–256), com gargalo de poll corrigido ([[f00-mvp]]).

## Gate

Gate A1–A5, o aceite da Fase 0 (`implementation/phase-0/phase-0-acceptance.md`). Estes são os critérios de aceite da fase; não são os itens A1–A3 do EP.

### A1 — Build

- [x] O workspace resolve de forma reprodutível: o CI compila a partir de um checkout limpo.
- [x] Controller, worker e CLI têm binários independentes: `ai-cluster-controller`, `ai-cluster-worker`, `ai-cluster`.
- [x] O protocolo é gerado a partir de definições `.proto`: o `build.rs` roda `tonic-prost-build` contra `proto/cluster.proto` com `protoc` vendorizado.

### A2 — Protocolo

- [x] O nó consegue se registrar: `RegisterNode` registra identidade e capabilities.
- [x] O controller confirma o registro: a resposta carrega `accepted` e o `cluster_id` atribuído.
- [x] O worker envia heartbeat a cada cinco segundos; uma janela de grace perdida vira o nó para `OFFLINE`.
- [x] O worker envia health report: escore de pressão de memória a cada tick.

### A3 — Identidade

- [x] A identidade do nó é independente do endereço IP: o `node_id` vem de `AI_CLUSTER_NODE_ID` ou do hostname, nunca do endereço de rede.
- [x] A identidade do dispositivo é separada da identidade lógica: o `device_id` lê `/etc/machine-id`, então sobrevive a um rename lógico e a uma reinstalação de imagem.

### A4 — Medição

- [x] O harness de benchmark mede um nó sem alterar seu escore de cluster à mão: `ai-cluster benchmark` — o controller registra o compute time reportado pelo worker e seu próprio round trip observado.
- [x] A saída do benchmark é versionada: cabeçalho `versao_benchmark=... escopo=... tamanho=... repeticoes=... semente=42` em toda execução.

### A5 — Sem LLM

- [x] A fase é completa sem qualquer runtime de modelo. O gate é o substrato distribuído, não a qualidade de inferência.

> [!todo] Pendente do gate
> A comparação entre duas máquinas físicas (critério de saída da fase) não foi rodada. Ver [[f00-runbook-duas-maquinas]] e [[f00-testes-iniciais]] (itens 4 e 6).

## Slices

| slice | conteúdo | estado |
|---|---|---|
| [[f00-estrutura-do-repositorio]] | workspace e fronteiras de crate | implementado |
| [[f00-mvp]] | MVP e primeiro alvo experimental (EP §20) | parcial |
| [[f00-runbook-duas-maquinas]] | procedimento de validação em duas máquinas | parcial |
| [[f00-testes-iniciais]] | snapshot da lista de testes da Fase 0 | superado |

## Gate padrão de fase

Código ✔ · benchmark ✔ (`ai-cluster benchmark`) · telemetria ✔ · teste de falha ✔ (saída graciosa, grafo) · documentação ✔ · ADR ✔ ([[adr-008-stack-inicial]]).

## Códigos antigos

- EP A1, A2, A3 (Fase 0); `roadmap 0`; `M0`.
- `phase-0/repository-structure`, `phase-0/two-machine-validation`, `phase-0/next-tests`, `phase-0/benchmark-sweep`, `implementation/mvp`, `phase-0/phase-0-acceptance`.

## Specs do sistema

- [[arquitetura]]
- [[protocolo-controller-worker]]
- [[mensagens]]
- [[worker-runtime]]
- [[work-unit]]
- [[adr-008-stack-inicial]]
