---
title: "Critérios de aceitação da Fase 0"
type: operational
status: living
tags: [operational, phase-0-acceptance]
---

# Critérios de aceitação da Fase 0

## A1 Build

- O workspace resolve de forma reprodutível. — ATENDIDO: o CI compila a partir de um checkout limpo.
- Controller, Worker e CLI têm binários independentes. — ATENDIDO:
  `ai-cluster-controller`, `ai-cluster-worker`, `ai-cluster`.
- O protocolo é gerado a partir de definições `.proto`. — ATENDIDO: o `build.rs` roda
  `tonic-prost-build` contra `proto/cluster.proto` com `protoc` vendorizado.

## A2 Protocolo

- O nó consegue se registrar. — ATENDIDO: `RegisterNode` registra identidade e capabilities.
- O controller confirma o registro. — ATENDIDO: a resposta carrega `accepted`
  e o `cluster_id` atribuído.
- O worker envia heartbeat. — ATENDIDO: intervalo de cinco segundos; uma janela de grace
  perdida vira o nó para `OFFLINE`.
- O worker envia health report. — ATENDIDO: escore de pressão de memória enviado a cada tick.

## A3 Identidade

- A identidade do nó é independente do endereço IP. — ATENDIDO: o `node_id` vem de
  `AI_CLUSTER_NODE_ID` ou do hostname, nunca do endereço de rede.
- A identidade do dispositivo é separada da identidade lógica do nó. — ATENDIDO: o `device_id`
  lê `/etc/machine-id`, então sobrevive a um rename lógico e a uma reinstalação de imagem.

## A4 Medição

- O harness de benchmark consegue medir um nó sem alterar seu escore de cluster manualmente. — ATENDIDO
  por `ai-cluster benchmark`; o controller registra o compute time reportado pelo worker e
  seu próprio round trip observado, então nenhum escore é digitado à mão.
- A saída do benchmark é versionada. — ATENDIDO pelo cabeçalho `versao_benchmark=... escopo=...
  tamanho=... repeticoes=... semente=42` impresso em toda execução.

## A5 Sem LLM

A fase é considerada completa sem qualquer runtime de modelo. O gate é o substrato distribuído, não a qualidade de inferência.
