---
title: "F09 — Transporte"
aliases: ["Fase E — Transport", "Fase E", "EP E1–E5"]
tags: [implementacao/fase/09, implementacao/estado/parcial]
type: implementacao/fase
status: parcial
codigos-antigos: ["EP E1–E5", "roadmap 2 (transport abstraction)"]
sistema: ["[[camada-de-transporte]]", "[[protocolo-controller-worker]]", "[[mensagens]]", "[[canal-de-fatias]]"]
---
# F09 — Transporte

## Objetivo

Separar a operação (Work Unit, fatia de modelo) do meio que a carrega. Uma API única (`send`, `receive`, `stream`, `probe`) deve permitir trocar TCP/gRPC por memória compartilhada intra-host, e depois por USB (F18), sem mudar a lógica da operação.

## Depende de

- [[fase-03-execucao-distribuida]]: Work Unit e DAG já transportados por gRPC.
- [[fase-08-inferencia]]: `ShardChannel` (canal residente de fatias) é o primeiro tráfego de latência sensível.

## Entrega

- Abstração de transporte com `send`, `receive`, `stream` e `probe`.
- Transporte por memória compartilhada para nós no mesmo host (E3).
- Medições de enlace além da vazão: latência, perda e congestionamento (E5).
- Critério de saída: a mesma Work Unit usa transportes diferentes sem mudança na operação.
- Fora desta fase: USB (E4), movido para [[fase-18-worker-movel]] (Q4).

## Checklist

Verificado em `code/crates/transport` e `code/crates/worker`, `code/crates/controller`.

- [~] E1 Abstração `send`/`receive`/`stream`/`probe`. O trait `Transport` e o `Link` genérico existem em `crates/transport`, com TCP e socket Unix e testes; o cluster ainda não passa por eles, os clientes gRPC são chamados direto ([[f09-abstracao-de-transporte]], [[adr-014-abstracao-de-transporte]]).
- [x] E2 TCP (gRPC sobre TCP). É o baseline de todo o tráfego: RPCs do controller, `ProbeLink`, `ShardChannel`.
- [ ] E3 Memória compartilhada. A variante `TransportKind::SharedMemory` está declarada, sem implementação; o meio `Unix` (intra-host, sem pilha de rede) é o degrau anterior.
- [~] E5 Métricas adaptativas de enlace. `ProbeLink` mede a vazão de upload até o controller (melhor de algumas rodadas, 2 MiB, referência 1 GbE) e alimenta `network_achieved_score`; a latência (cinco probes vazios, o menor) vai em `HealthReport.network_latency_ms` ([[f09-metricas-de-enlace]]). Perda e congestionamento não são medidos.
- [x] Critério de saída: a mesma Work Unit serializada passa por transportes distintos (TCP e Unix) sem alterar a operação, com o custo de cada um medido (`examples/bench.rs`, [[adr-014-abstracao-de-transporte]]). Vale para a camada de transporte; o caminho de Work Unit do cluster segue em gRPC.

> [!warning] Tetos atuais
> - O probe mede só o upload e só o caminho até o controller; não há perna de download.
> - Não há seleção de transporte: o cluster sempre usa gRPC/TCP, inclusive entre processos do mesmo host.
> - `TransportKind::Usb` existe no enum sem uso; não deve ser lido como suporte.

## Gate

Gate padrão (EP §19): código, benchmarks, telemetria, testes de falha, atualização de documentação e ADR quando a arquitetura mudar. Não avançar só porque o código compila.

Gate específico (manifesto): mesma Work Unit executada em dois transportes distintos, com a medição de custo de cada um.

## Slices

Nenhum slice criado. A pasta `09-transporte/` tem só esta nota (ela também é o MOC da pasta).

Slices a criar:

- `f09-abstracao-de-transporte`: E1. Trait com `send`, `receive`, `stream`, `probe`; gRPC como primeira implementação.
- `f09-memoria-compartilhada`: E3. Transporte intra-host, com a mesma operação do TCP.
- `f09-metricas-de-enlace`: E5. Latência, perda e congestionamento, além da vazão já medida pelo probe.

Evidência existente de E5: `code/tests/phase-c-network.sh`.

## Códigos antigos

- EP E1–E5. E4 (USB) é F18/Q4.
- Roadmap 2, item "transport abstraction" (`[~]`: gRPC direto, sem camada de abstração).

## Specs do sistema

- [[camada-de-transporte]]: contrato de transporte-alvo.
- [[protocolo-controller-worker]] e [[mensagens]]: RPCs que a abstração deve cobrir.
- [[canal-de-fatias]]: canal residente que usa o transporte.
