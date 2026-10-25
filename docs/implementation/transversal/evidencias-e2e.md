---
title: "Evidências E2E"
aliases: ["Evidências", "code/tests"]
tags: [implementacao/transversal, implementacao/evidencias]
type: implementacao/transversal
status: em-andamento
---

# Evidências E2E

Cada script em `code/tests/` prova um critério de um slice. A tabela liga o script ao que ele verifica e ao slice e à fase que o declaram como evidência. Os testes de unidade e de paridade estão em [[estrategia-de-testes]].

Rodar a partir de `code/`: `bash tests/<script>.sh`. Os scripts `phase-j-*` precisam dos pesos em `code/models/`.

## Scripts

| Script | O que prova | Slice | Fase |
|---|---|---|---|
| `phase-1-leave.sh` | nó sai com SIGTERM e o sobrevivente termina cada unidade, sem nada encalhado | [[f01-entrada-e-saida-de-nos]] | [[fase-01-nucleo-do-cluster]] |
| `phase-b4-node-show.sh` | `ai-cluster node show <id>` mostra compute, storage, pressão de memória e reserva | [[f01-entrada-e-saida-de-nos]] | [[fase-01-nucleo-do-cluster]] |
| `phase-2-graph.sh` | diamante A → (B, C) → D: B e C só despachados após A; D após B e C | [[f03-grafo-de-dependencias]] | [[fase-03-execucao-distribuida]] |
| `phase-2-failure.sh` | worker morto sem saída graciosa: unidades reenfileiradas, DAG termina | [[f03-tolerancia-a-falhas]] | [[fase-03-execucao-distribuida]] |
| `scaling-sweep.sh` | 1/2/3 workers × tamanhos 32–256: compute vs transferência por unidade (primeiro experimento, §20) | [[f03-varredura-compute-vs-transferencia]] | [[fase-03-execucao-distribuida]] |
| `phase-2-placement.sh` | unidade com `min_cpu_threads` é recusada ao nó fraco e espera o nó capaz | [[f04-placement]] | [[fase-04-escalonamento]] |
| `phase-g-memory.sh` | unidade só vai a nó com memória para o footprint (derivado da própria unidade) | [[f04-placement]] | [[fase-04-escalonamento]] |
| `phase-g-placement.sh` | custo decide o placement: mesma unidade roda no nó capaz (18.000 µs) e não no fraco (2.020.000 µs) | [[f04-placement]] | [[fase-04-escalonamento]] |
| `phase-g-cores.sh` | o scheduler local concede cores e o worker os usa (~7× menos compute com 4 cores que com 1) | [[f04-scheduler-local]] | [[fase-04-escalonamento]] |
| `phase-g-priority.sh` | unidade de prioridade 5 enfileirada por último completa antes das de prioridade 0 | [[f04-scheduler-local]] | [[fase-04-escalonamento]] |
| `phase-g-slots.sh` | worker com 4 slots termina o lote em menos da metade do tempo de 1 slot | [[f04-scheduler-local]] | [[fase-04-escalonamento]] |
| `phase-g-quarantine.sh` | worker que mente no checksum é posto em `QUARANTINED` e o trabalho termina no honesto | [[f04-quarentena]] | [[fase-04-escalonamento]] |
| `phase-g-reservations.sh` | nó retido recusa unidades de outras requisições e roda as do dono, mesmo sendo o pior | [[f04-reservas]] | [[fase-04-escalonamento]] |
| `phase-c-benchmark.sh` | adicionar um nó produz perfil com compute medido, não só contagem de threads | [[f02-benchmark-de-capability]] | [[fase-02-capability-e-saude]] |
| `phase-c-current.sh` | perfil com current + confiança; a medida instável tem confiança menor | [[f02-capability-corrente]] | [[fase-02-capability-e-saude]] |
| `phase-c-network.sh` | link nominal descoberto e usado pelo placement | [[f02-descoberta-de-rede]] | [[fase-02-capability-e-saude]] |
| `phase-c4-health.sh` | nó quente tem saúde pior e a unidade vai para o frio, com capability idêntica | [[f02-modelo-de-saude]] | [[fase-02-capability-e-saude]] |
| `phase-c-storage-health.sh` | disco degradado tem saúde pior e a unidade vai para o nó saudável, com capability idêntica | [[f02-modelo-de-saude]] | [[fase-02-capability-e-saude]] |
| `phase-c-network-degrade.sh` | link estrangulado com `tc` depois do registro derruba `rede_atual` e a unidade vai para o nó intacto (exige `apt` no contêiner) | [[f02-descoberta-de-rede]] | [[fase-02-capability-e-saude]] |
| `phase-h-metrics.sh` | agregado de métricas muda com o cluster e bate com as partes; `/metrics` | [[f05-metricas]] | [[fase-05-observabilidade]] |
| `phase-h-inference-metrics.sh` | três gerações aparecem no `/metrics` do gateway com TTFT, ITL, tokens/s e KV cache | [[f05-metricas-de-inferencia]] | [[fase-05-observabilidade]] |
| `phase-h-traces.sh` | `request_id` liga requisição às unidades, com nó e tempos de compute e transferência | [[f05-traces]] | [[fase-05-observabilidade]] |
| `phase-h-predictions.sh` | previsão por unidade (score fixado) e soma no roll-up | [[f05-previsto-vs-real]] | [[fase-05-observabilidade]] |
| `phase-h-dashboard.sh` | painel mostra nós, fila e reservas a partir de três chamadas do controller | [[f05-dashboard]] | [[fase-05-observabilidade]] |
| `phase-i-state.sh` | estado sobrevive a interrupção abrupta; perder o primário carrega a cópia ao lado | [[f06-persistencia-e-checkpoint]] | [[fase-06-estado-e-recuperacao]] |
| `phase-i-events.sh` | a linha do tempo de eventos sobrevive ao restart do controller | [[f06-eventos-persistidos]] | [[fase-06-estado-e-recuperacao]] |
| `phase-i-checkpoint.sh` | snapshot proporcional ao trabalho que falta (8 unidades de 256³: 4.194.410 B → 2 B) | [[f06-persistencia-e-checkpoint]] | [[fase-06-estado-e-recuperacao]] |
| `phase-i4-recovery.sh` | unidade em voo atravessa restart do controller; posse no disco; dono ausente → reoferecida | [[f06-recuperacao]] | [[fase-06-estado-e-recuperacao]] |
| `phase-i4-replica.sh` | fila sobrevive à perda dos dois arquivos locais, recuperada da cópia de um nó | [[f06-recuperacao]] | [[fase-06-estado-e-recuperacao]] |
| `phase-i-storage.sh` | nó classifica armazenamento em WARM (SSD/NVMe) e COLD (HDD) lendo `/sys/block` | [[f10-classes-de-armazenamento]] | [[fase-10-inteligencia-de-storage]] |
| `phase-s1-join-token.sh` | controller fail-closed sem token; só aceita quem apresenta o token em toda chamada | [[f07-token-de-join]] | [[fase-07-seguranca-base]] |
| `phase-s2-approval.sh` | nó novo fica pendente sem trabalho até `approve`; aprovação sobrevive a re-registro | [[f07-aprovacao-de-no]] | [[fase-07-seguranca-base]] |
| `phase-s4-tls.sh` | gRPC com TLS e CA; http puro e CA errada recusados; sem certificado só com `AI_CLUSTER_INSECURE=1` | [[f07-tls]] | [[fase-07-seguranca-base]] |
| `phase-s5-persist.sh` | aprovação persiste em restarts; `revoke` devolve o nó a pendente e derruba o worker | [[f07-aprovacao-de-no]] | [[fase-07-seguranca-base]] |
| `phase-j-inference.sh` | gateway autentica por chave, lista modelo, `/v1/embeddings` sobre matmul nos workers, erros estáveis | [[f08-gateway-e-api]] | [[fase-08-inferencia]] |
| `phase-j-chat.sh` | `/v1/chat/completions` (`gpt2`, `smollm2`, `instruct`): continuação determinística, 401 sem chave | [[f08-chat-completions]] | [[fase-08-inferencia]] |
| `phase-j-shards.sh` | projeções da SmolLM2 em 2 workers com fatias próprias; mesma continuação; 503 sem worker | [[f08-canal-residente-de-fatias]] | [[fase-08-inferencia]] |

## Lacunas

Slices declarados sem script próprio, ou sem prova E2E:

- [[f07-operador-e-credencial-por-no]] (S3a/S3b): coberto só indiretamente pelos scripts S1, S2 e S5; sem roteiro dedicado.
- [[f00-mvp]] e [[f00-runbook-duas-maquinas]]: a prova entre máquinas físicas não existe; `scaling-sweep.sh` roda com containers no mesmo host.
- [[f02-descoberta-de-rede]] e [[fase-09-transporte]]: sem E2E que degrade o link (cenário de degradação de rede).
- [[f04-scheduler-local]]: sem prova de filas de recurso, prioridades ou GPU (não implementados).
- [[f10-classes-de-armazenamento]]: sem prova de políticas por classe (K4 e K5 não iniciados).
- [[f05-dashboard]]: o painel não lê TTFT, ITL, tokens/s ou KV cache; a prova deles está em `phase-h-inference-metrics.sh`, no `/metrics` do gateway.
- Fases F11 a F19: nenhum script.
