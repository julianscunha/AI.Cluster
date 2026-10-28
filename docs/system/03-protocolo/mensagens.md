---
title: "Mensagens do protocolo"
aliases: ["protocol/messages", "Protocol Messages", "Mensagens"]
tags: [sistema/protocolo]
type: sistema/protocolo
---

# Mensagens do protocolo

Catálogo de serviços, RPCs e mensagens do schema `code/proto/cluster.proto` (pacote `ai.cluster.v1`). O schema é a referência; esta nota é o índice, com quem chama, quem pode chamar e os limites. O número entre parênteses é o do campo. Índice: [[protocolo]]. Interações e identidade: [[protocolo-controller-worker]].

## Serviços e RPCs

Autorização: **join** = token de join (todo nó e cliente); **op** = token de operador; **inf** = token de inferência; **nó** = credencial do nó, além do token de join (o operador também passa).

| RPC | Serviço | Request → Response | Chamador | Autorização | Efeito |
|---|---|---|---|---|---|
| `ProbeLink` | `LinkProbe` | `ProbeLinkRequest` → `ProbeLinkResponse` | worker | join | recebe o payload e devolve quantos bytes chegaram |
| `RegisterNode` | `ClusterControl` | `RegisterNodeRequest` → `RegisterNodeResponse` | worker | join | registra o nó e devolve `cluster_id` e `node_credential` |
| `ApproveNode` | `ClusterControl` | `ApproveNodeRequest` → `ApproveNodeResponse` | operador | op | torna confiável um nó registrado; tira o nó de quarentena |
| `RevokeNode` | `ClusterControl` | `RevokeNodeRequest` → `RevokeNodeResponse` | operador | op | retira a confiança e a credencial; devolve à fila o que estava em voo |
| `SetRelease` | `ClusterControl` | `SetReleaseRequest` → `SetReleaseResponse` | operador | op | define a release desejada do cluster; `version` em branco cancela o rollout |
| `GetOutput` | `ClusterControl` | `GetOutputRequest` → `GetOutputResponse` | operador | op | devolve a saída guardada de uma requisição com `keep_result` |
| `ShardChannel` | `ClusterControl` | stream `ShardResult` → stream `ShardCompute` | worker | join + nó | canal residente de fatias; o worker abre |
| `ShardMatmul` | `ClusterControl` | `ShardMatmulRequest` → `ShardMatmulResponse` | gateway | inf ou op | `x × fatia^T` em cada nó indicado |
| `Heartbeat` | `ClusterControl` | `HeartbeatRequest` → `HeartbeatResponse` | worker | join + nó | batida de vida; leva e recebe a época da réplica de estado e, quando há, a release a instalar |
| `ReportHealth` | `ClusterControl` | `HealthStreamRequest` → `HealthStreamResponse` | worker | join + nó | saúde e, opcionalmente, o perfil remedido. O nome é histórico: a RPC é unária |
| `ListNodes` | `ClusterControl` | `ListNodesRequest` → `ListNodesResponse` | operador ou CLI | join | snapshot dos nós, sem credencial |
| `LeaveNode` | `ClusterControl` | `LeaveNodeRequest` → `LeaveNodeResponse` | worker | join + nó | o nó sai: fica `DRAINING`, devolve o que tinha em voo e é removido |
| `RequestWork` | `ClusterControl` | `RequestWorkRequest` → `RequestWorkResponse` | worker | join + nó | puxa uma unidade, respeitando reservas e `min_cpu_threads` |
| `SubmitWorkResult` | `ClusterControl` | `SubmitWorkResultRequest` → `SubmitWorkResultResponse` | worker | join + nó | entrega o resultado de uma unidade |
| `EnqueueWork` | `ClusterControl` | `EnqueueWorkRequest` → `EnqueueWorkResponse` | operador | op | enfileira `repetitions` unidades independentes |
| `EnqueueGraph` | `ClusterControl` | `EnqueueGraphRequest` → `EnqueueGraphResponse` | operador | op | enfileira um grafo sintético de quatro unidades com dependência; recusa quando a fila passa de 256 unidades |
| `CancelRequest` | `ClusterControl` | `CancelRequestRequest` → `CancelRequestResponse` | operador | op | retira da fila as unidades da requisição que ainda não foram despachadas e as que dependiam delas |
| `WorkReport` | `ClusterControl` | `WorkReportRequest` → `WorkReportResponse` | operador ou CLI | join | concluídas, pendentes e falhas |
| `ListEvents` | `ClusterControl` | `ListEventsRequest` → `ListEventsResponse` | operador ou CLI | join | eventos recentes do controller |
| `WatchEvents` | `ClusterControl` | `WatchEventsRequest` → stream de `Event` | operador ou CLI | operador | barramento: cada evento novo, assim que é gravado; 16 assinantes; assinante lento recebe `events_lagged` |
| `GetMetrics` | `ClusterControl` | `GetMetricsRequest` → `MetricsResponse` | operador, exportador | join | roll-up do cluster no instante |
| `GetTrace` | `ClusterControl` | `GetTraceRequest` → `GetTraceResponse` | operador ou CLI | join | caminho requisição → unidades |
| `Reserve` | `ClusterControl` | `ReserveRequest` → `ReserveResponse` | operador | op | retém um nó para uma requisição |
| `ListReservations` | `ClusterControl` | `ListReservationsRequest` → `ListReservationsResponse` | operador ou CLI | join | reservas vivas |
| `ReleaseReservation` | `ClusterControl` | `ReleaseReservationRequest` → `ReleaseReservationResponse` | operador | op | libera a reserva do nó |

Total: 2 serviços e 24 RPCs.

## Nó e identidade

| Mensagem | Campos (número) | Notas |
|---|---|---|
| `NodeIdentity` | `node_id` (1), `device_id` (2), `architecture` (3), `os_version` (4), `runtime_version` (5), `image_sha256` (6), `can_update` (7) | `image_sha256`: SHA-256 medido do slot em execução; vazio até a medição terminar. `can_update`: o nó sabe aplicar uma release; só esses entram no rollout e só eles podem ser canário |
| `CapabilityReport` | `cpu_threads` (1), `memory_bytes` (2), `compute_score` (3), `memory_bandwidth_score` (4), `storage_score` (5), `network_score` (6), `precisions` (7), `accelerators` (8), `storage_classes` (9), `current_compute_score` (10), `compute_confidence` (11), `compute_median_score` (12), `compute_deviation` (13), `network_achieved_score` (14), `current_network_score` (15), `work_slots` (16), `storage_latency_ms` (17), `shard_cache_bytes` (18), `baseline_confidence` (19), `network_retransmit_ratio` (20), `network_jitter` (21), `network_jitter_measured` (26: sonda com duas ou mais repetições; sem isso, jitter 0 é "não medido"), `storage_read_score` (22), `storage_device` (23), `memory_bandwidth_loaded_score` (24), `storage_devices` (25, mensagem `StorageDevice`) | Scores relativos a uma referência; zero é "não medido", nunca "nulo". `compute_score` é o melhor caso e ordena os nós; `compute_median_score` calibra previsões. `network_score` é a velocidade nominal do link (1.0 = 1 GbE); `network_achieved_score` é a taxa alcançada até o controller, e o placement usa a menor das duas. `storage_classes` vai do mais quente ao mais frio (`nvme`, `ssd`, `hdd`) |
| `HealthReport` | `health_score` (1), `cpu_utilization` (2), `memory_utilization` (3), `temperature_c` (4), `storage_health` (5), `network_latency_ms` (6), `thermal_throttling` (7), `swap_utilization` (8), `disk_io_ops` (9), `battery_level` (10), `on_battery` (11) | `disk_io_ops` é zero no primeiro relatório. `battery_level` zero com `on_battery` falso significa "sem bateria visível", não "bateria vazia" |
| `NodeSnapshot` | `identity` (1), `capability` (2), `health` (3), `state` (4), `registered_at_unix_ms` (5), `last_heartbeat_unix_ms` (6), `trusted` (7), `attested` (8) | `state` é texto (`READY`, `OFFLINE`, `DRAINING`…). `attested` é calculado na leitura: a versão e a medida do slot batem com a release desejada. A credencial não sai aqui |
| `RegisterNodeRequest` | `identity` (1), `capability` (2), `replica` (3) | `replica` é a cópia de estado que o nó guarda |
| `RegisterNodeResponse` | `accepted` (1), `cluster_id` (2), `message` (3), `node_credential` (4) | a credencial é emitida a cada registro de quem não prova a anterior |
| `HeartbeatRequest` | `node_id` (1), `unix_ms` (2), `replica_epoch_unix_ms` (3) | época zero quando o nó não guarda cópia. O `unix_ms` do nó é informativo: a vivacidade usa o relógio do controller |
| `HeartbeatResponse` | `acknowledged` (1), `replica` (2), `release` (3), `cancel_work_ids` (4) | `cancel_work_ids`: unidades em voo canceladas por `CancelRequest`, o nó para a conta (até uma batida de atraso). `replica` só vem quando a época do controller avançou; `release` só vem quando difere da versão do nó e o rollout o liberou |
| `Release` | `version` (1), `url` (2), `sha256` (3), `image_sha256` (4), `signature` (5) | release do sistema. `sha256` é do artefato; `image_sha256` é o do conteúdo do slot depois de gravado; `signature` é Ed25519 em base64 (88 caracteres), conferida pelo nó |
| `SetReleaseRequest` / `SetReleaseResponse` | `release` (1) / — | ver [[versionamento]] para a validação e o rollout |
| `HealthStreamRequest` | `node_id` (1), `health` (2), `capability` (3) | `capability` é opcional; quando vem, substitui o perfil guardado e `current_compute_score` carrega a medida mais recente |
| `HealthStreamResponse` | `accepted` (1) | |
| `StateReplica` | `epoch_unix_ms` (1), `snapshot` (2) | `WorkSnapshot` codificado, com os mesmos bytes do arquivo de estado do controller |
| `ListNodesRequest` / `ListNodesResponse` | — / `nodes` (1) | |
| `ApproveNodeRequest` / `ApproveNodeResponse` | `node_id` (1) / `approved` (1) | nó desconhecido responde `approved` falso; aprovar de novo um nó já aprovado continua verdadeiro |
| `RevokeNodeRequest` / `RevokeNodeResponse` | `node_id` (1) / `revoked` (1), `requeued` (2) | nó desconhecido responde `revoked` falso |
| `LeaveNodeRequest` / `LeaveNodeResponse` | `node_id` (1) / `accepted` (1), `requeued` (2) | |
| `ProbeLinkRequest` / `ProbeLinkResponse` | `payload` (1) / `received_bytes` (1) | payload de até 4 MiB |

## Trabalho

| Mensagem | Campos (número) | Notas |
|---|---|---|
| `WorkUnit` | `work_id` (1), `rows` (2), `shared` (3), `cols` (4), `matrix_a` (5), `matrix_b` (6), `depends_on` (7), `matrix_a_from` (8), `matrix_b_from` (9), `min_cpu_threads` (10), `request_id` (11), `priority` (12) | `*_from` substitui o operando pela saída de outra unidade no despacho ([[work-unit]]). `priority` é a classe, de 0 a 9 |
| `WorkResult` | `work_id` (1), `node_id` (2), `rows` (3), `cols` (4), `matrix_c` (5), `checksum` (6), `compute_time_us` (7) | |
| `RequestWorkRequest` / `RequestWorkResponse` | `node_id` (1) / `has_work` (1), `work` (2) | |
| `SubmitWorkResultRequest` / `SubmitWorkResultResponse` | `result` (1) / `accepted` (1), `message` (2) | |
| `EnqueueWorkRequest` / `EnqueueWorkResponse` | `rows` (1), `shared` (2), `cols` (3), `seed` (4), `repetitions` (5), `min_cpu_threads` (6), `request_id` (7), `priority` (8), `require_attested` (9) / `enqueued` (1), `request_id` (2) | `request_id` vazio faz o controller gerar um, que volta na resposta |
| `EnqueueGraphRequest` / `EnqueueGraphResponse` | `size` (1), `seed` (2), `request_id` (3), `keep_result` (4), `priority` (5) / `enqueued` (1), `blocked` (2), `request_id` (3) | `keep_result` guarda a saída final para `GetOutput`; o controller guarda as últimas 64 |
| `GetOutputRequest` / `GetOutputResponse` | `request_id` (1) / `found` (1), `rows` (2), `cols` (3), `matrix_c` (4) | `found` falso para id desconhecido, não terminado ou expulso |
| `CancelRequestRequest` / `CancelRequestResponse` | `request_id` (1) / `removed` (1), `in_flight` (2) | `removed` conta unidades retiradas da fila; `in_flight` conta as que já estão num worker e terminam normalmente |
| `WorkReportRequest` / `WorkReportResponse` | `limit` (1; só as últimas N, 0 = todas) / `completed` (1, lista de `WorkReportEntry`), `pending` (2), `failed` (3) | `failed` são unidades que esgotaram o orçamento de tentativas, ou de cuja dependência isso aconteceu |
| `WorkReportEntry` | `work_id` (1), `node_id` (2), `compute_time_us` (3), `transfer_time_us` (4), `total_latency_us` (5), `checksum` (6) | |
| `WorkLease` | `work_id` (1), `node_id` (2), `dispatched_at_unix_ms` (3), `predicted_compute_us` (4), `unit` (5) | posse de uma unidade em voo; `unit` vai junto para o controller poder reoferecê-la |
| `WorkOutput` | `work_id` (1), `matrix` (2) | saída de uma unidade concluída |
| `WorkSnapshot` | `pending` (1), `outputs` (2), `attempts` (3, mapa), `failed` (4), `sequence` (5), `dispatched` (6), `epoch_unix_ms` (7) | estado que o controller grava em disco. Não é enviado por cliente; ver [[modelo-de-dados]] |

## Fatias (canal residente)

| Mensagem | Campos (número) | Notas |
|---|---|---|
| `ShardOp` | `node_id` (1), `model_subdir` (2), `tensor` (3), `row_start` (4), `row_end` (5), `by_columns` (6) | uma faixa de uma projeção; `by_columns` indica faixa de colunas de um tensor `[in, out]` |
| `ShardMatmulRequest` | `rows` (1), `k` (2), `x` (3), `ops` (4) | `x` é `rows × k`, linha a linha; igual para todas as operações |
| `ShardOutput` | `rows` (1), `cols` (2), `data` (3), `compute_us` (4) | `compute_us` é medido no worker, sem a rede |
| `ShardMatmulResponse` | `outputs` (1) | mesma ordem de `ops` |
| `ShardCompute` | `op_id` (1), `model_subdir` (2), `tensor` (3), `row_start` (4), `row_end` (5), `rows` (6), `k` (7), `x` (8), `by_columns` (9) | controller → worker; `op_id` é gerado pelo controller |
| `ShardResult` | `op_id` (1), `ok` (2), `error` (3), `rows` (4), `cols` (5), `data` (6), `compute_us` (7), `invalid` (8) | worker → controller; `invalid` marca recusa do pedido, não falha do nó |

Comportamento de cada campo e limites: [[canal-de-fatias]].

## Rastreio, eventos e métricas

| Mensagem | Campos (número) | Notas |
|---|---|---|
| `TraceEntry` | `work_id` (1), `node_id` (2), `status` (3), `compute_time_us` (4), `transfer_time_us` (5), `total_latency_us` (6), `predicted_compute_us` (7) | `status` é `queued`, `dispatched`, `completed` ou `failed`. Só unidade concluída tem tempos |
| `GetTraceRequest` / `GetTraceResponse` | `request_id` (1) / `request_id` (1), `units` (2) | id desconhecido devolve resposta vazia, sem erro |
| `Event` | `at_unix_ms` (1), `kind` (2), `detail` (3) | tempo do controller |
| `ListEventsRequest` / `ListEventsResponse` | `limit` (1) / `events` (1) | `limit` 0 devolve tudo que ainda está retido |
| `MetricsResponse` | `nodes_total` (1), `nodes_available` (2), `units_completed` (3), `units_pending` (4), `units_blocked` (5), `units_failed` (6), `total_compute_time_us` (7), `total_latency_us` (8), `avg_compute_time_us` (9), `avg_latency_us` (10), `p50_compute_time_us` (11), `p95_compute_time_us` (12), `p50_latency_us` (13), `p95_latency_us` (14), `total_predicted_compute_us` (15) | snapshot do instante, não série temporal. Percentis pelo índice `ceil(p × n) − 1` |

## Reservas

| Mensagem | Campos (número) | Notas |
|---|---|---|
| `Reservation` | `node_id` (1), `request_id` (2), `expires_at_unix_ms` (3) | expira sozinha |
| `ReserveRequest` / `ReserveResponse` | `node_id` (1), `request_id` (2), `ttl_ms` (3), `slots` (4) / `granted` (1), `expires_at_unix_ms` (2), `message` (3) | `ttl_ms` 0 usa 60 s; `slots` 0 retém o nó inteiro |
| `ListReservationsRequest` / `ListReservationsResponse` | — / `reservations` (1) | |
| `ReleaseReservationRequest` / `ReleaseReservationResponse` | `node_id` (1) / `released` (1) | um nó tem no máximo uma reserva |

> [!warning] Campos novos e compatibilidade
> `by_columns` (`ShardOp` 6, `ShardCompute` 9) e `invalid` (`ShardResult` 8) mudam o resultado quando ausentes. Um worker que não os conhece calcula faixas de GPT-2 como linhas e pode devolver forma compatível, sem erro. Por isso controller e workers se atualizam juntos ([[versionamento]]).
>
> `priority` (`WorkUnit` 12, `EnqueueWorkRequest` 8, `EnqueueGraphRequest` 5) e a atualização de nós (`can_update`, `image_sha256`, `HeartbeatResponse.release`) são seguros numa mistura de versões: um peer antigo usa o padrão, e um nó sem `can_update` fica de fora do rollout.

> [!info] Planejado
> Negociação de features no registro; `WorkAssignment` com tipo de operação, precisão e URIs de tensor (hoje a unidade é uma multiplicação `f64` fixa); mensagens próprias para carregar e descarregar modelos e fatias, para checkpoint e restauração e para cancelar unidade individual; telemetria por stream.
