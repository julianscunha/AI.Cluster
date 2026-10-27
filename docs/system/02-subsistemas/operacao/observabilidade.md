---
title: "Observabilidade e telemetria"
aliases: ["Observabilidade", "Telemetria", "Métricas", "Eventos", "Traces"]
tags: [sistema/subsistema/operacao]
type: sistema/subsistema
---

# Observabilidade e telemetria

O cluster responde três perguntas em qualquer momento: como está cada nó e o cluster inteiro, o que mudou, e o que aconteceu com uma requisição específica. A telemetria tem três tipos, cada um com um papel, e uma comparação entre previsão e medida para calibrar o modelo de custo. Voltar para [[operacao]].

## Três tipos de telemetria

| Tipo | Responde | Forma | Onde se lê |
|---|---|---|---|
| Métricas | Como está agora | Totais, médias, percentis e sinais por nó | `ai-cluster metrics`, `ai-cluster nodes`, `/metrics` |
| Eventos | O que mudou | Linha do tempo de transições | `ai-cluster events` |
| Traces | Por onde passou uma requisição | Unidades de uma requisição, com nó e tempos | `ai-cluster request trace <id>` |

O painel `ai-cluster dashboard` junta os três numa tela só ([[cli-tui]]).

## Métricas do cluster

`ai-cluster metrics` imprime `chave=valor`, com os mesmos nomes dos campos do protocolo:

```text
nodes_total=1 nodes_available=1
units_completed=2 units_pending=0 units_blocked=0 units_failed=0
total_compute_time_us=1217 avg_compute_time_us=608 total_predicted_compute_us=36
total_latency_us=3000 avg_latency_us=1500
p50_compute_time_us=538 p95_compute_time_us=679
p50_latency_us=1000 p95_latency_us=2000
```

- `nodes_available` é o conjunto que o placement enxerga: registrado e não drenando, nem offline, nem em quarentena.
- `units_pending` inclui as unidades em voo. `units_blocked` é o subconjunto que espera dependência do grafo.
- `units_failed` são as unidades que esgotaram o orçamento de tentativas e nunca vão completar.
- `total_compute_time_us` vem do próprio worker, no tempo de compute que ele informa. `total_latency_us` vem do relógio do controller, do despacho ao resultado.
- A diferença entre latência e compute é transferência, fila e espera de consulta. Numa unidade que termina em menos de um milissegundo, o tempo do worker pode passar do tempo observado pelo controller, porque os relógios são diferentes.
- Média é divisão inteira, arredondada para baixo, e vale zero quando nada foi concluído.
- Percentis usam o método nearest-rank: a resposta é sempre um tempo que aconteceu de verdade. Com uma unidade só, p50 e p95 têm o mesmo valor.

Média e percentil respondem perguntas diferentes. A média mostra o custo típico. O p95 mostra o que a cauda sente, e a cauda é o que um cliente que espera por uma requisição experimenta.

## Métricas de nó

O worker lê `/proc` e `/sys` a cada heartbeat e envia:

- **CPU:** utilização entre duas leituras de `/proc/stat`. Iowait conta como ocioso.
- **Swap:** `1 - SwapFree/SwapTotal`, de `/proc/meminfo`.
- **Temperatura:** de `/sys/class/thermal`, em grau Celsius.
- **IOPS:** leituras e escritas concluídas por segundo, de `/proc/diskstats`.
- **Score de rede:** velocidade negociada do link sobre a referência de 1 GbE. É um teto nominal, não a vazão sob carga.

Da medição do registro (capability) saem também, no `nodes` da CLI e no exporter: `baseline_confidence` (baseline limpo ou perturbado), `network_retransmit_ratio` e `network_jitter` (perda e oscilação da sonda de rede), `storage_read_score` (leitura com `O_DIRECT`) e `memory_bandwidth_loaded_score` (memória com os núcleos juntos).

O roll-up do controller (`GetMetrics` e o exporter) traz também as médias de rede e storage dos nós prontos que mediram e as unidades concluídas por segundo em 1 e 5 minutos (zero sem história que cubra a janela). O controller lembra as últimas 100 mil unidades concluídas; as contagens de concluídas passam a valer essa janela quando ela enche.

Zero significa "não medido", não "zero". Na primeira leitura, CPU e IOPS são zero. Sem sensor de temperatura, a temperatura é zero.

O `health_score` reflete só a pressão de memória. Os sinais de CPU, temperatura e swap são informados crus.

Em contêiner, `/proc` e `/sys` são do host inteiro. CPU e disco mostram o estado da máquina, não o consumo do processo.

## Métricas de inferência

O gateway mede cada geração e expõe as séries em `GET /metrics` (chave de API no cabeçalho `Authorization: Bearer`):

- `requests_total`, falhas, tokens de prompt e tokens gerados.
- **TTFT** (tempo até o primeiro token, prompt incluído), com p50 e p95.
- **ITL** (intervalo entre tokens seguintes), com p50 e p95.
- **Tokens por segundo** de geração.
- **Tamanho do KV cache**, em bytes, da última geração e a maior vista: posições vezes camadas vezes largura de K e V vezes 8 bytes.
- Cache de respostas: acertos, erros, tokens poupados e entradas ([[edge-optimizer]]).
- Por tenant: tokens cobrados e pedidos recusados ([[request-manager]]).

Percentis são nearest-rank, sobre as últimas 1024 amostras de cada série. Cancelamento e prazo estourado contam como falha.

## Inferência no painel

O gateway se apresenta ao controller a cada 15 s e leva junto o que mediu (`GatewayStats`): contadores de requisições, falhas e tokens, TTFT e ITL (p50 e p95 das últimas 1024 amostras), taxa de decodificação, tamanho do cache KV, acertos e erros do cache de respostas e **tokens evitados** pelo cache. `ai-cluster dashboard` mostra uma seção INFERÊNCIA por gateway, sem falar HTTP com ele.

- **Taxas e tendência:** requisições e tokens gerados por segundo nas janelas de 1 e 5 minutos, com a razão como tendência (`↑`, `→`, `↓`). Zero antes de a história cobrir a janela; nunca extrapolado.
- **Gateway sem sinal:** o painel diz há quantos segundos e avisa que os números são do último sinal.
- **Contadores sobrevivem ao reinício:** com `AI_CLUSTER_GATEWAY_STATE` (ou `AI_CLUSTER_DATA_DIR`, que usa `gateway-<id>.state`) o gateway grava os contadores a cada apresentação e os recarrega ao subir. Percentis, tamanho do KV e as entradas do cache não persistem: descrevem a carga recente. Dois gateways não podem dividir o mesmo arquivo.
- **Controller antigo:** sem `ListGateways`, o painel omite a seção.

## Métricas do controller em HTTP

O exportador do controller serve `GET /metrics` na porta `AI_CLUSTER_METRICS_ADDR` (padrão `127.0.0.1:9101`). Ele exige `Authorization: Bearer <token de operador>`.

- Nomes começam com `ai_cluster_`. Os demais seguem o nome do campo de `metrics`, para que um número visto no scraper e na CLI seja o mesmo.
- Cada nó vira uma série com o rótulo `node=`.
- Se o exportador não sobe, o erro é reportado e o controller segue funcionando.

## Eventos

`ai-cluster events [--limit N] [--follow]` imprime do mais antigo para o mais recente. Padrão: os últimos 20. `--limit 0` mostra tudo que o controller ainda guarda.

Nomes de evento:

| Evento | Quando |
|---|---|
| `node_registered` | Nó se registrou. |
| `node_approved`, `node_revoked` | Aprovação e revogação de nó. |
| `node_left` | Nó saiu do cluster ao desligar ou ao drenar. |
| `node_offline` | Nó sem heartbeat além da carência. |
| `node_reserved`, `reservation_released` | Reserva de nó criada ou liberada. |
| `work_enqueued`, `graph_enqueued` | Trabalho entrou na fila. |
| `work_completed` | Unidade concluída. |
| `request_cancelled` | Unidades de uma requisição foram removidas da fila. |
| `shard_channel_opened` | Worker abriu o canal de fatias. |
| `rollout` | Mudança de estado do rollout de release. |
| `audit` | Ação administrativa. Ver adiante. |

Os eventos são guardados em anel de 256 entradas. O tempo é sempre o do controller, porque relógios de nó não são confiáveis para ordenação. Com `AI_CLUSTER_EVENTS=<arquivo>`, a linha do tempo sobrevive a reinício: o arquivo guarda uma linha por evento e é reescrito quando passa de 1024 linhas.

## Trilha de auditoria

Cada ação administrativa gera um evento `audit` com ação, alvo e endereço de quem chamou. Ações cobertas: aprovação, revogação, enfileiramento de trabalho e de grafo, cancelamento de requisição, reserva, liberação de reserva e definição de release.

- Tentativa negada por token errado não aparece na trilha. Ela falha antes de qualquer ação.
- A ação é registrada antes do efeito. Um `approve` de nó inexistente aparece como tentativa.
- Eventos `audit` não são expulsos do anel por eventos comuns.
- O ator é o endereço de rede de quem chamou, não uma identidade. Não há CN de certificado nem nome de operador.

## Previsto contra real

O controller estima o tempo de compute de cada unidade com o modelo de custo. Ele soma as estimativas nas métricas (`total_predicted_compute_us`), ao lado do tempo medido. A diferença mostra o erro do modelo no cluster.

Por nó, o controller também aprende a razão entre o real e o previsto. Ela corrige as previsões seguintes do mesmo nó, com taxa de aprendizado de 0,3. Razões fora de 0,05 a 20 são descartadas como erro de relógio. A correção afeta a previsão, não o placement: o nó escolhido continua pela taxa de referência do benchmark.

A razão não é persistida. Reiniciar o controller reaprende.

## Traces por requisição

`ai-cluster request trace <id>` lista as unidades de uma requisição:

```text
REQUISIÇÃO abc123 UNIDADES 2
TRABALHO   NÓ             ESTADO      COMPUTE_US TRANSFER_US   TOTAL_US
w1         node-t         completed       12345         678      13023
w2         node-t         completed       11987         654      12641
unidades=2 compute_somado_us=24332 transfer_somado_us=1332 latencia_somada_us=25664
```

Estados de uma unidade:

- `queued`: na fila, ainda não despachada. Inclui as que esperam dependência do grafo.
- `dispatched`: em voo num nó, sem resultado.
- `completed`: concluída. É a única com tempos.
- `failed`: esgotou as tentativas.

O id da requisição é escolhido pelo cliente, ou cunhado pelo controller quando o cliente não manda. Id desconhecido devolve trace vazio, sem erro.

## Painel

`ai-cluster dashboard [--watch N]` mostra nós disponíveis, a tabela de nós, o trabalho (concluídas, pendentes, bloqueadas e falhas), os tempos de compute e latência com o previsto, e as reservas. Sem `--watch` imprime uma vez. Com `--watch N` repinta a cada N segundos. O painel lê as mesmas chamadas da CLI e não tem navegação.

## Limites conhecidos

- **Sem histórico.** Métricas são instantâneas. Totais cobrem só as unidades que o controller ainda retém: quando uma sai do anel de conclusões, o número cai. Taxa por segundo exige série temporal, que não existe.
- **Métricas de inferência são da memória do gateway.** Reiniciar o gateway zera as séries, e o painel do controller não as mostra.
- **Eventos e traces se perdem no reinício,** exceto a linha do tempo, que persiste quando `AI_CLUSTER_EVENTS` está definido. O id de requisição de uma unidade não sobrevive ao reinício do controller.
- **Eventos sem fsync.** Uma queda de energia pode perder os últimos eventos.
- **Arquivo de eventos sem proteção.** Não tem assinatura, e quem edita o arquivo edita a trilha.
- **Trace sem spans internos.** Mostra o custo por unidade, não o tempo dentro do worker.
- **Sinais de rede e armazenamento incompletos.** Latência de rede, banda de memória e saúde de armazenamento não são medidos. O score de rede é nominal.
- **IOPS conta o disco e as partições.** O número é um teto.
- **Sem diagnóstico.** O cluster não indica causa provável nem ação recomendada.

> Planejado: métricas de nó de frequência, carga, throttling, GPU e NPU, saúde de armazenamento e banda de memória.
> Planejado: eventos `NODE_DEGRADED`, `SSD_WARNING` e `REBALANCE_STARTED`.
> Planejado: amostragem adaptativa, com frequência alta só em nó degradado ou anômalo.
> Planejado: diagnóstico com causa provável, impacto e ação.
> Planejado: histórico persistido de métricas e exportação para um coletor.
