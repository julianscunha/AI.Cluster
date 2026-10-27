---
title: "Motor de capacidade"
aliases: ["Motor de capacidade", "Capability Engine"]
tags: [sistema/subsistema/substrato]
type: sistema/subsistema
---

# Motor de capacidade

O motor de capacidade mede o que cada nó consegue fazer e converte o resultado em um perfil que o controller usa para escolher o nó de cada unidade. Não existe score global: cada dimensão fica separada, porque um nó pode ser forte em compute e fraco em rede.

## Dimensões medidas

| Dimensão | Campos do relatório | Como é medida |
|---|---|---|
| Compute | `compute_score`, `compute_median_score`, `compute_deviation`, `compute_confidence` | Multiplicação de matrizes 256×256 em `f64`, 5 execuções. O score é a melhor execução, por core, normalizado por 6,0 GFLOP/s (um core de referência vale 1,0). |
| Compute corrente | `current_compute_score`, `compute_confidence` | Remedido com o nó ocioso a cada 60 s (`AI_CLUSTER_RECOMPUTE_INTERVAL_SECS`). |
| Rede nominal | `network_score` | Velocidade negociada do link, relativa a 1 GbE. |
| Rede alcançada | `network_achieved_score`, `current_network_score` | Sonda de upload: 3 envios de 2 MiB ao controller, melhor execução, timeout de 5 s. Remedida no ocioso. |
| Latência de rede | `HealthReport.network_latency_ms` | Menor de 5 sondas vazias, worker até o controller. Remedida no ocioso. |
| Armazenamento | `storage_classes`, `storage_score`, `storage_latency_ms` | Classes `nvme`, `ssd` e `hdd`. Escrita de 16 MiB com `fsync` (melhor de 3), relativa a 500 MB/s. Latência de escrita de 4 KiB com `fsync`. Mede só o disco do diretório temporário. |
| Memória | `memory_bytes` | `MemTotal` de `/proc/meminfo`. |
| Banda de memória | `memory_bandwidth_score` | Cópia de 32 MiB, melhor de 3, relativa a 10 GB/s. Reportada, sem uso no placement. |
| Threads | `cpu_threads` | Threads reportadas, limitadas às do host. |

Um valor zero significa "não medido" em todos os campos. O placement ignora o zero em vez de lê-lo como nó ruim. Um nó sem medida de compute usa como estimativa 1,0 por thread.

## Como o benchmark de compute funciona

- A medida usa o mesmo kernel que executa as unidades. Assim, o número que o controller usa para decidir é o número que o worker entrega.
- Por core, para que um nó de 8 threads não pareça 8 vezes maior só por medir o paralelismo.
- O melhor entre as execuções é a capacidade, porque a contenção só deixa a execução mais lenta. A mediana e o desvio relativo (coeficiente de variação) descrevem o que de fato se espera.
- A confiança é `melhor / mediana`: 1,0 quando as execuções concordam, mais perto de zero quando a medição foi perturbada. Uma medição quebrada tem confiança zero.

## Medida corrente e previsão

- O ranking dos nós usa o baseline (a medida de registro), para não reordenar candidatos a cada tick.
- A previsão de tempo de compute usa a medida corrente, quando existe, porque pergunta quanto a unidade vai demorar agora.
- A previsão usa a mediana como taxa típica quando o desvio é de até 0,25 (`DEVIATION_TRUST_LIMIT`). Acima disso, volta ao melhor caso.
- Somado à taxa, há um custo fixo por unidade de 600 µs (`AI_CLUSTER_UNIT_SETUP_US`).
- Cada nó ainda aprende uma correção: a média móvel (peso 0,3) da razão entre o tempo real e o previsto, limitada entre 0,05 e 20. A correção fica em memória e é reaprendida depois de reinício.

## Fixar valores

Para testes que não devem depender da velocidade do host:

| Variável | Efeito |
|---|---|
| `AI_CLUSTER_COMPUTE_SCORE` | Fixa o compute (valor positivo). O nó não mede compute, reporta a corrente e a mediana como "não medido" e a previsão usa o baseline. |
| `AI_CLUSTER_NETWORK_SCORE` | Fixa a rede nominal. O nó não faz sonda de rede. |
| `AI_CLUSTER_STORAGE_HEALTH` | Fixa a saúde de armazenamento (ver [[saude-e-confianca]]). |
| `AI_CLUSTER_RECOMPUTE_INTERVAL_SECS` | Cadência das remedições no ocioso (padrão 60). |

## Limites conhecidos

- A medida é de uma passada de cinco execuções, não de um histórico. Um nó ocioso vota com o que mediu naquela janela.
- A confiança é carregada, não ponderada: a previsão usa a medida corrente com confiança baixa do mesmo jeito que com confiança alta.
- A remedição só ocorre com o nó ocioso. Um nó que nunca fica ocioso reporta só o baseline.
- O compute é assumido como linear com o número de threads. Turbo e SMT deixam a previsão otimista em nós com muitas threads.
- A confiança do baseline não é modelada. O número de registro é uma leitura instantânea.
- A medida de compute é feita em `f64`, mas o campo `precisions` reporta só `fp32`. O perfil não distingue a precisão de cálculo.
- Memória, armazenamento e rede não têm remedição corrente equivalente à do compute. Armazenamento só mede o disco do diretório temporário, e a rede só o upload até o controller.
- A banda de memória é medida em um só thread, o que subestima nós com vários canais de memória.

> Planejado: perfil multidimensional completo (int4, int8, fp16, GPU, NPU, `thermal_headroom`), medidas de aceleradores e dimensões de capacidade dinâmica (pressão de memória, carga, política de energia) no próprio perfil.
