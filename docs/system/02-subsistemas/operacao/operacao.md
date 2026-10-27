---
title: "Operação"
aliases: ["Operação", "Operar o cluster"]
tags: [sistema/moc, sistema/subsistema/operacao]
type: sistema/moc
---

# Operação

Subsistema que mostra o cluster a quem opera: métricas, eventos, traces e o painel, e a CLI que dá acesso a tudo isso. Voltar para [[subsistemas]].

## Notas

- [[observabilidade]]: os três tipos de telemetria (métricas, eventos e traces), previsto contra real e a trilha de ações administrativas.
- [[cli-tui]]: a CLI `ai-cluster`, o painel de uma tela e os comandos de operação.

Leitura: [[observabilidade]] primeiro, depois [[cli-tui]]. A CLI é a interface de todo dado da observabilidade.

## Portas

| Serviço | Endereço padrão | Variável | Protocolo |
|---|---|---|---|
| Controller (gRPC) | `0.0.0.0:50051` | `AI_CLUSTER_ADDR` | gRPC com TLS |
| Métricas do controller | `127.0.0.1:9101` | `AI_CLUSTER_METRICS_ADDR` | HTTP, Bearer de operador |
| Gateway de inferência | `127.0.0.1:8080` | `AI_CLUSTER_GATEWAY_ADDR` | HTTP, chave de API |

O worker não expõe porta. Ele abre o canal ao controller por iniciativa própria. Para raspar métricas de outra máquina, o exportador precisa ouvir em `0.0.0.0:9101`, e o canal fica em HTTP puro.

## Arquivos de estado do controller

| Variável | O que guarda | Sem a variável |
|---|---|---|
| `AI_CLUSTER_NODES` | Nós, credenciais e aprovação (modo 0600, em claro). | Registro só na memória. Nós voltam a pendentes após reinício. |
| `AI_CLUSTER_STATE` | Fila de trabalho e unidades em voo. | Fila só na memória. |
| `AI_CLUSTER_EVENTS` | Linha do tempo de eventos. | Eventos só na memória. |
| `AI_CLUSTER_RELEASE_STATE` | Release desejada pelo rollout. | Rollout some no reinício. |

## Comportamento do controller

- **Heartbeat.** Um nó sem heartbeat por `AI_CLUSTER_HEARTBEAT_GRACE_MS` (padrão 15000 ms) é marcado offline. As unidades em voo dele voltam à fila, ou falham se esgotaram as tentativas.
- **Reinício.** Unidades cujo dono não voltou depois de um reinício também voltam à fila, depois do mesmo prazo de carência.
- **Varredura.** A checagem de heartbeat roda a cada 5 s.
- **Fail-closed.** O controller não sobe sem token de join, sem token de operador, ou sem certificado TLS (a menos que `AI_CLUSTER_INSECURE=1`).

## Rotina de operação

1. `ai-cluster dashboard` para o estado geral.
2. `ai-cluster nodes` para aprovação, versão e medida de cada nó.
3. `ai-cluster events` para a linha do tempo recente.
4. `ai-cluster metrics` ou o scraper em `/metrics` para números agregados.
5. `ai-cluster request trace <id>` para uma requisição específica.

Cada comando está em [[cli-tui]].

## Limites conhecidos

- **Sem histórico.** Métricas e eventos são instantâneos ou um anel em memória, com um arquivo de eventos opcional. Não há série temporal.
- **Sem acesso de leitura separado.** A CLI usa sempre o token de operador, inclusive para ler.
- **Rede do exportador sem TLS.** O token de operador viaja em claro se o exportador for exposto na rede.

> Planejado: seções de inferência no painel e nos comandos de operação.
