---
title: "ADR-021 — Persistência de traces, estado do gateway e exportação de eventos"
aliases: ["ADR-021", "traces", "syslog"]
tags: [sistema/adr, subsistema/observabilidade]
type: sistema/adr
---

# ADR-021 — Persistência de traces, estado do gateway e exportação de eventos

Os comandos citados seguem a sintaxe do [[adr-024-interface-de-comandos-unica-aic]].

Status: decidido · Data: 2026-10-27 · Voltar para [[decisoes]].

## Contexto

Três limites da observabilidade (F05): os traces e o roll-up de unidades concluídas zeravam a cada reinício do controller, os contadores de inferência do gateway também, e os eventos só podiam ser lidos pela CLI.

## Decisão

1. **Traces em um arquivo só de acréscimo.** Um registro `CompletedRecord` (prost, com prefixo de tamanho) por unidade concluída, em `AI_CLUSTER_TRACES` ou `traces.log` no diretório de dados. No início o controller lê o arquivo, fica com os últimos 100 mil registros e o reescreve limpo; durante a execução só acrescenta. Ao passar de 100 mil registros o arquivo é rodado para `.1` por um `rename`, de modo que nada é reescrito sob o lock da fila e o disco guarda no máximo 200 mil registros. Uma cauda truncada por queda é descartada na leitura. Não vai no snapshot nem na réplica: seria reescrito a cada mutação.
2. **Estado do gateway reaproveita `GatewayStats`.** O gateway grava só os contadores (requisições, falhas, tokens, acertos e erros do cache, tokens evitados) com `tmp` + `rename` a cada apresentação (15 s) e os recarrega ao subir. Percentis, KV e entradas do cache não persistem, porque descrevem a carga recente. O padrão do arquivo contém o id do gateway, para dois gateways não se corromperem.
3. **O gateway leva suas estatísticas ao controller** em `ReportGateway`, e `aic cluster dashboard` as mostra. A CLI não fala HTTP com o gateway.
4. **Eventos saem por syslog UDP (RFC 5424).** `AI_CLUSTER_EVENTS_SYSLOG=host:porta` liga uma tarefa que assina o barramento (`EventLog::subscribe`) e envia um datagrama por evento: facility `local0`, severidade notice para `audit` e info para o resto, texto sem controle e truncado em 1000 bytes.

## Cuidados com o arquivo de traces

- Um registro sem `work_id` (o tamanho zero de um arquivo estendido com zeros após uma queda) é tratado como corrupção: a leitura para ali, em vez de povoar o histórico com unidades fantasmas.
- A reescrita na abertura faz `fsync` antes do `rename`. Se ela falha, o log fica desligado nessa execução em vez de acrescentar atrás de lixo. Um `work_id` repetido é ignorado, o que protege de um `.1` que não pôde ser removido.
- O acréscimo acontece fora do lock da fila, e uma rotação que não consegue reabrir o arquivo tenta de novo worker próximo registro.

## Consequências

- O `units_completed` e os percentis do roll-up passam a atravessar reinícios (até 100 mil unidades). Uma unidade falha continua perdendo o `request_id` no reinício.
- UDP não confirma entrega nem criptografa: um coletor fora do ar só perde eventos, e os eventos `audit` (com IPs) saem em texto claro. Serve a uma rede de laboratório ou a um relay local.
- A taxa de unidades por segundo precisa de história: logo depois de um reinício sem arquivo ela vale zero até a janela se cobrir.

## Alternativas

- Guardar as concluídas no `WorkSnapshot`: descartada. O snapshot é reescrito inteiro a cada mutação e iria também para a réplica I4.
- Webhook HTTP para o coletor: adiado. Exige um cliente HTTP e política de repetição; o syslog cobre os coletores comuns sem dependência nova.
- Prometheus remote-write para os contadores do gateway: descartada. O gateway já expõe `/metrics` para quem faz scrape.
