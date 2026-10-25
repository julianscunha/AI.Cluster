---
title: "Mensagens do protocolo"
aliases: ["protocol/messages", "Protocol Messages", "Mensagens"]
tags: [sistema/protocolo]
type: sistema/protocolo
status: vivo
implementacao: parcial
fases: ["[[fase-00-fundacao]]", "[[fase-02-capability-e-saude]]", "[[fase-03-execucao-distribuida]]", "[[fase-04-escalonamento]]", "[[fase-05-observabilidade]]", "[[fase-06-estado-e-recuperacao]]", "[[fase-07-seguranca-base]]", "[[fase-08-inferencia]]"]
---

# Mensagens do protocolo

Catálogo de serviços, RPCs e mensagens de `code/proto/cluster.proto` (pacote `ai.cluster.v1`). O `.proto` é a autoridade; esta nota é o índice, com quem chama, quem pode chamar e os limites. O campo número entre parênteses é o da mensagem no schema. Índice: [[protocolo]]. Interações e identidade: [[protocolo-controller-worker]].

## Serviços e RPCs

Autorização: **join** = token de join (todo nó e cliente); **op** = token de operador; **inf** = token de inferência; **nó** = credencial do nó, além do join.

| RPC | Serviço | Request → Response | Chamador | Autorização | Efeito |
|---|---|---|---|---|---|
| `ProbeLink` | `LinkProbe` | `ProbeLinkRequest` → `ProbeLinkResponse` | worker | join | recebe o payload e devolve quantos bytes chegaram (C2) |
| `RegisterNode` | `ClusterControl` | `RegisterNodeRequest` → `RegisterNodeResponse` | worker | join | registra o nó e devolve `cluster_id` e `node_credential` |
| `ApproveNode` | `ClusterControl` | `ApproveNodeRequest` → `ApproveNodeResponse` | operador | op | aprova um nó desconhecido de confiança (S2) |
| `RevokeNode` | `ClusterControl` | `RevokeNodeRequest` → `RevokeNodeResponse` | operador | op | revoga o nó e reenfileira o que tinha em voo |
| `GetOutput` | `ClusterControl` | `GetOutputRequest` → `GetOutputResponse` | operador | op | devolve a saída guardada de uma requisição com `keep_result` (J1) |
| `ShardChannel` | `ClusterControl` | stream `ShardResult` → stream `ShardCompute` | worker | join + nó | canal residente de fatias; o worker abre (J5) |
| `ShardMatmul` | `ClusterControl` | `ShardMatmulRequest` → `ShardMatmulResponse` | gateway | inf ou op | `x × fatia^T` em cada nó indicado (J5) |
| `Heartbeat` | `ClusterControl` | `HeartbeatRequest` → `HeartbeatResponse` | worker | join + nó | batida de vida; leva e recebe época de réplica de estado (I4) |
| `ReportHealth` | `ClusterControl` | `HealthStreamRequest` → `HealthStreamResponse` | worker | join + nó | saúde e, opcionalmente, o perfil remedido (C3). O nome é legado: a RPC é unária |
| `ListNodes` | `ClusterControl` | `ListNodesRequest` → `ListNodesResponse` | operador ou CLI | join | snapshot dos nós, sem credencial |
| `LeaveNode` | `ClusterControl` | `LeaveNodeRequest` → `LeaveNodeResponse` | worker | join + nó | nó sai: passa a `DRAINING`, reenfileira e é removido |
| `RequestWork` | `ClusterControl` | `RequestWorkRequest` → `RequestWorkResponse` | worker | join + nó | puxa uma unidade, respeitando reservas e `min_cpu_threads` |
| `SubmitWorkResult` | `ClusterControl` | `SubmitWorkResultRequest` → `SubmitWorkResultResponse` | worker | join + nó | entrega o resultado de uma unidade |
| `EnqueueWork` | `ClusterControl` | `EnqueueWorkRequest` → `EnqueueWorkResponse` | operador | op | enfileira `repetitions` unidades independentes |
| `EnqueueGraph` | `ClusterControl` | `EnqueueGraphRequest` → `EnqueueGraphResponse` | operador | op | enfileira o grafo diamante sintético (quatro unidades com dependência); recusa com a fila acima de 256 |
| `WorkReport` | `ClusterControl` | `WorkReportRequest` → `WorkReportResponse` | operador ou CLI | join | concluídas, pendentes e falhas |
| `ListEvents` | `ClusterControl` | `ListEventsRequest` → `ListEventsResponse` | operador ou CLI | join | últimos eventos do ciclo de vida |
| `GetMetrics` | `ClusterControl` | `GetMetricsRequest` → `MetricsResponse` | operador, exportador | join | roll-up do cluster no instante (H1) |
| `GetTrace` | `ClusterControl` | `GetTraceRequest` → `GetTraceResponse` | operador ou CLI | join | caminho requisição → unidades (H3) |
| `Reserve` | `ClusterControl` | `ReserveRequest` → `ReserveResponse` | operador | op | retém um nó para uma requisição (G6) |
| `ListReservations` | `ClusterControl` | `ListReservationsRequest` → `ListReservationsResponse` | operador ou CLI | join | reservas vivas |
| `ReleaseReservation` | `ClusterControl` | `ReleaseReservationRequest` → `ReleaseReservationResponse` | operador | op | libera a reserva do nó |

Total: 2 serviços e 22 RPCs.

## Mensagens por domínio

### Nó e identidade

| Mensagem | Campos (número) | Notas |
|---|---|---|
| `NodeIdentity` | `node_id` (1), `device_id` (2), `architecture` (3), `os_version` (4), `runtime_version` (5) | sem campo de features |
| `CapabilityReport` | `cpu_threads` (1), `memory_bytes` (2), `compute_score` (3), `memory_bandwidth_score` (4), `storage_score` (5), `network_score` (6), `precisions` (7), `accelerators` (8), `storage_classes` (9), `current_compute_score` (10), `compute_confidence` (11), `compute_median_score` (12), `compute_deviation` (13), `network_achieved_score` (14), `current_network_score` (15) | `*_score` relativos à referência; zero é "não medido", nunca "nulo". Ver [[f02-benchmark-de-capability]], [[f02-capability-corrente]], [[f02-descoberta-de-rede]] |
| `HealthReport` | `health_score` (1), `cpu_utilization` (2), `memory_utilization` (3), `temperature_c` (4), `storage_health` (5), `network_latency_ms` (6), `thermal_throttling` (7), `swap_utilization` (8), `disk_io_ops` (9) | `disk_io_ops` é zero no primeiro relatório |
| `NodeSnapshot` | `identity` (1), `capability` (2), `health` (3), `state` (4), `registered_at_unix_ms` (5), `last_heartbeat_unix_ms` (6), `trusted` (7) | o estado é texto (`READY`, `OFFLINE`, `DRAINING`…); a credencial não sai aqui |
| `RegisterNodeRequest` | `identity` (1), `capability` (2), `replica` (3) | `replica` é a cópia de estado que o nó guarda (I4) |
| `RegisterNodeResponse` | `accepted` (1), `cluster_id` (2), `message` (3), `node_credential` (4) | a credencial é emitida a quem não prova a anterior (S3b) |
| `HeartbeatRequest` | `node_id` (1), `unix_ms` (2), `replica_epoch_unix_ms` (3) | época zero quando o nó não guarda cópia |
| `HeartbeatResponse` | `acknowledged` (1), `replica` (2) | `replica` só vem quando a época do controller avançou |
| `HealthStreamRequest` | `node_id` (1), `health` (2), `capability` (3) | `capability` opcional; quando vem, substitui o perfil guardado |
| `HealthStreamResponse` | `accepted` (1) | |
| `StateReplica` | `epoch_unix_ms` (1), `snapshot` (2) | `WorkSnapshot` codificado, mesmos bytes dos arquivos locais (I4) |
| `ListNodesRequest` / `ListNodesResponse` | — / `nodes` (1) | |
| `ApproveNodeRequest` / `ApproveNodeResponse` | `node_id` (1) / `approved` (1) | aprovar de novo um nó já aprovado continua verdadeiro |
| `RevokeNodeRequest` / `RevokeNodeResponse` | `node_id` (1) / `revoked` (1), `requeued` (2) | |
| `LeaveNodeRequest` / `LeaveNodeResponse` | `node_id` (1) / `accepted` (1), `requeued` (2) | |
| `ProbeLinkRequest` / `ProbeLinkResponse` | `payload` (1) / `received_bytes` (1) | payload de até 4 MiB |

### Trabalho

| Mensagem | Campos (número) | Notas |
|---|---|---|
| `WorkUnit` | `work_id` (1), `rows` (2), `shared` (3), `cols` (4), `matrix_a` (5), `matrix_b` (6), `depends_on` (7), `matrix_a_from` (8), `matrix_b_from` (9), `min_cpu_threads` (10), `request_id` (11) | `*_from` substitui o operando pela saída de outra unidade no despacho ([[work-unit]]) |
| `WorkResult` | `work_id` (1), `node_id` (2), `rows` (3), `cols` (4), `matrix_c` (5), `checksum` (6), `compute_time_us` (7) | |
| `RequestWorkRequest` / `RequestWorkResponse` | `node_id` (1) / `has_work` (1), `work` (2) | |
| `SubmitWorkResultRequest` / `SubmitWorkResultResponse` | `result` (1) / `accepted` (1), `message` (2) | |
| `EnqueueWorkRequest` / `EnqueueWorkResponse` | `rows`, `shared`, `cols`, `seed`, `repetitions`, `min_cpu_threads`, `request_id` (1–7) / `enqueued` (1), `request_id` (2) | `request_id` vazio faz o controller gerar um |
| `EnqueueGraphRequest` / `EnqueueGraphResponse` | `size` (1), `seed` (2), `request_id` (3), `keep_result` (4) / `enqueued` (1), `blocked` (2), `request_id` (3) | `keep_result` guarda a saída final para `GetOutput`, até 64 requisições |
| `GetOutputRequest` / `GetOutputResponse` | `request_id` (1) / `found` (1), `rows` (2), `cols` (3), `matrix_c` (4) | `found` falso para id desconhecido, não terminado ou expulso |
| `WorkReportRequest` / `WorkReportResponse` | — / `completed` (1, lista de `WorkReportEntry`), `pending` (2), `failed` (3) | `failed` são unidades que esgotaram o orçamento de tentativas |
| `WorkReportEntry` | `work_id`, `node_id`, `compute_time_us`, `transfer_time_us`, `total_latency_us` (1–5), `checksum` (6) | |
| `WorkSnapshot` | `pending` (1), `outputs` (2), `attempts` (3, mapa), `failed` (4), `sequence` (5), `dispatched` (6), `epoch_unix_ms` (7) | estado persistido do controller ([[f06-persistencia-e-checkpoint]]) |
| `WorkLease` | `work_id` (1), `node_id` (2), `dispatched_at_unix_ms` (3), `predicted_compute_us` (4), `unit` (5) | posse de unidade em voo; `unit` vai junto para poder reoferecer |
| `WorkOutput` | `work_id` (1), `matrix` (2) | saída de unidade concluída |

### Fatias (canal residente)

| Mensagem | Campos (número) | Notas |
|---|---|---|
| `ShardOp` | `node_id` (1), `model_subdir` (2), `tensor` (3), `row_start` (4), `row_end` (5), `by_columns` (6) | uma faixa de uma projeção; `by_columns` = faixa de colunas de `[in, out]` |
| `ShardMatmulRequest` | `rows` (1), `k` (2), `x` (3), `ops` (4) | `x` é `rows × k`, linha a linha; igual para todas as operações |
| `ShardOutput` | `rows` (1), `cols` (2), `data` (3), `compute_us` (4) | `compute_us` é medido no worker, sem rede |
| `ShardMatmulResponse` | `outputs` (1) | mesma ordem de `ops` |
| `ShardCompute` | `op_id` (1), `model_subdir` (2), `tensor` (3), `row_start` (4), `row_end` (5), `rows` (6), `k` (7), `x` (8), `by_columns` (9) | controller → worker; `op_id` é do controller |
| `ShardResult` | `op_id` (1), `ok` (2), `error` (3), `rows` (4), `cols` (5), `data` (6), `compute_us` (7), `invalid` (8) | worker → controller; `invalid` é recusa do pedido, não do nó |

Comportamento de cada campo e limites: [[canal-de-fatias]].

### Rastreio, eventos e métricas

| Mensagem | Campos (número) | Notas |
|---|---|---|
| `TraceEntry` | `work_id` (1), `node_id` (2), `status` (3), `compute_time_us` (4), `transfer_time_us` (5), `total_latency_us` (6), `predicted_compute_us` (7) | `status` é `queued`, `dispatched`, `completed` ou `failed` |
| `GetTraceRequest` / `GetTraceResponse` | `request_id` (1) / `request_id` (1), `units` (2) | vazio, sem erro, para id desconhecido (perdido por restart) |
| `Event` | `at_unix_ms` (1), `kind` (2), `detail` (3) | tempo do controller; ring de 256 eventos |
| `ListEventsRequest` / `ListEventsResponse` | `limit` (1) / `events` (1) | `limit` 0 devolve tudo que ainda está retido |
| `MetricsResponse` | `nodes_total`…`units_failed` (1–6), `total_compute_time_us` (7), `total_latency_us` (8), `avg_*` (9–10), `p50_/p95_*` compute e latency (11–14), `total_predicted_compute_us` (15) | snapshot, não série temporal |

### Reservas

| Mensagem | Campos (número) | Notas |
|---|---|---|
| `Reservation` | `node_id` (1), `request_id` (2), `expires_at_unix_ms` (3) | expira sozinha |
| `ReserveRequest` / `ReserveResponse` | `node_id` (1), `request_id` (2), `ttl_ms` (3) / `granted` (1), `expires_at_unix_ms` (2), `message` (3) | `ttl_ms` 0 usa 60 s |
| `ListReservationsRequest` / `ListReservationsResponse` | — / `reservations` (1) | |
| `ReleaseReservationRequest` / `ReleaseReservationResponse` | `node_id` (1) / `released` (1) | um nó tem no máximo uma reserva |

> [!warning] Campos novos e compatibilidade
> `by_columns` (`ShardOp` 6, `ShardCompute` 9) e `invalid` (`ShardResult` 8) são campos novos. Em proto3 um peer antigo ignora o número desconhecido e usa o padrão (`false`). Para `by_columns` isso é **silenciosamente errado**: um worker antigo corta a faixa como linhas de um tensor `[in, out]` e pode devolver forma compatível com a checagem do controller. Por isso, campo com efeito semântico novo exige que controller e worker sejam atualizados juntos ([[versionamento]]). `invalid` ausente faz o controller tratar a recusa como falha do nó, o que é seguro mas põe o nó de lado à toa.

> [!info] O que já existe
> - Tudo acima está implementado em `code/crates/controller` e `code/crates/worker`; os testes unitários ficam nesses crates.
> - Slices: [[f00-mvp]] (WorkUnit e matriz), [[f01-entrada-e-saida-de-nos]], [[f02-benchmark-de-capability]], [[f02-capability-corrente]], [[f02-descoberta-de-rede]], [[f03-tolerancia-a-falhas]], [[f03-grafo-de-dependencias]], [[f04-placement]], [[f04-reservas]], [[f05-eventos]], [[f05-metricas]], [[f05-traces]], [[f06-persistencia-e-checkpoint]], [[f07-token-de-join]], [[f07-aprovacao-de-no]], [[f07-operador-e-credencial-por-no]], [[f08-canal-residente-de-fatias]], [[f08-gateway-e-api]].

> [!todo] Mensagens da proposta original que não existem no `.proto`
> - Campos de `NodeIdentity` para features (`features`, negociação no registro).
> - `WorkAssignment` com `plan_id`, `type` (`MATMUL`), `precision` e URIs `tensor://`: hoje a unidade é uma multiplicação `f64` fixa com `matrix_a`/`matrix_b`.
> - `HealthReport.state` (`DEGRADED`): o estado vive em `NodeSnapshot.state`, não na saúde enviada pelo worker.
> - `TELEMETRY`, `LOAD_MODEL`, `UNLOAD_MODEL`, `CANCEL_WORK`, `CHECKPOINT`, `RESTORE` como mensagens próprias.
