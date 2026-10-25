---
title: "F02 — Modelo de saúde do nó"
aliases: ["health-model", "C4"]
tags: [implementacao/fase/02, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-02-capability-e-saude]]"
codigos-antigos: ["C4", "health-model"]
sistema: ["[[saude-e-confianca]]"]
evidencia: ["code/tests/phase-c4-health.sh", "code/tests/phase-c-storage-health.sh"]
---
# F02 — Modelo de saúde do nó

- [ ] Superado

## Objetivo

O `health_score` do nó era `1.0 - memória_usada` desde o começo: um nó com metade da RAM
livre mas rodando a 95 °C reportava a mesma saúde de um nó frio. Esta é a fatia C4: o score
passa a levar os domínios que o worker **mede**, e o placement já usa o resultado.

## O que foi feito

- `thermal_pressure` / `health_score` no worker, puras e testáveis.
- `temperature_c()` ganhou `AI_CLUSTER_TEMPERATURE_C`, no mesmo molde dos outros overrides:
  contêiner comum não expõe `/sys/class/thermal`, então sem o override o termo térmico não
  pode ser exercitado numa máquina de desenvolvimento.
- Nada mudou no controller: o `placement_cost` já escalava o custo total pelo score
  (`HEALTH_WEIGHT`), então fazer o score enxergar mais domínios bastou para o escalonador
  passar a evitá-los.

Sem dependência nova: `max`, `clamp` e uma subtração.

## Semântica

O score é **o pior domínio**, não a média. Um nó sem swap e com a memória em 90% está em
apuros; a média com um domínio saudável esconderia exatamente o domínio que importa. Hoje
entram quatro domínios medidos:

| domínio | medida | onde |
| --- | --- | --- |
| pressão de memória | `1 - MemAvailable/MemTotal` | `/proc/meminfo` |
| pressão de swap | `1 - SwapFree/SwapTotal` | `/proc/meminfo` |
| pressão térmica | rampa de 80 °C a 100 °C | `/sys/class/thermal` ou override |
| saúde de storage | `1 - atual/baseline` da taxa de escrita com fsync | sonda do registro e remedida no ocioso |

**CPU não entra.** O [[saude-e-confianca]] lista CPU como domínio de saúde e o score a deixa de fora
porque utilização mede *carga*, não *condição*: um nó que acabou de terminar uma unidade lê
como ocupado no exato instante em que vira o melhor lugar para a próxima. O placement já
pergunta isso — ele rankeia candidatos ociosos — então carga pertence ao modelo de custo,
não ao de saúde. O `cpu_utilization` continua no relatório, cru, para quem souber julgar.

**Sem sensor não é nó frio, mas também não é nó quente.** Temperatura 0 significa "nenhum
sensor visível" em contêiner — a mesma convenção de um compute score não medido. O score não
pune o que não consegue ver; o valor cru fica no relatório.

**Piso em 0.05, não em 0.** O placement lê um score exatamente 0 como "não reportado" e
ignora a penalidade (`placement.rs:104`). Sem piso, um nó com as piores leituras possíveis
pareceria um nó sem leitura nenhuma e não pagaria nada — o inverso do pretendido. Nós mortos
já saem por estado.

## Evidência

Dois nós com capability idêntica (2 threads, 1 GiB, compute 1.0), só a temperatura diferente:

```
NÓ                   ESTADO     ARQ        THREADS    RAM_GB COMPUTE    SAÚDE    CPU_%  TEMP_C  SWAP_% ARMAZ
node-cool            READY      x86_64           2      1.00    1.00     0.78     1.60   45.00    1.14 hdd
node-hot             READY      x86_64           2      1.00    1.00     0.05     4.69  100.00    1.14 hdd
```

`node-cool` fica em 0.78 — o termo de memória do host, já que os dois leem o mesmo
`/proc/meminfo` — e `node-hot` no piso, porque a 100 °C a pressão térmica satura. Com a
capability igual, os termos de compute, transferência e fila do modelo de custo se cancelam
e a diferença de custo é só a saúde: a unidade única roda em `node-cool`.

O E2E é `bash code/tests/phase-c4-health.sh`; os testes de unidade são
`thermal_pressure_ramps_to_one_at_the_critical_temperature`,
`the_worst_domain_decides_the_health_score` e `a_measured_node_never_scores_as_unreported`
em `worker/src/main.rs`.

## Limites

**Limiares de temperatura genéricos.** 80 °C e 100 °C valem para um núcleo de servidor; um
núcleo de laptop ferve antes. O [[saude-e-confianca]] pede uma baseline por hardware e o worker ainda
não tem como saber qual é — a detecção de hardware é a fase C1.

**Saúde de storage é só lentidão.** A taxa de escrita com fsync é remedida no ocioso (cadência do compute) e comparada com o baseline do registro; `storage_health` = atual/baseline (razão ≥ 0,8 conta como 1.0, por causa do ruído do fsync), 1.0 = como ao entrar, 0 = não medido (fora da conta). `AI_CLUSTER_STORAGE_HEALTH` fixa o valor, como a temperatura. Mede só o dispositivo do tmp e não vê desgaste nem erros (SMART), e o [[f10-classes-de-armazenamento]] ainda não mede latência. Evidência: `bash code/tests/phase-c-storage-health.sh` (saúde 0.80 × 0.20, unidade vai para o nó saudável); coberta também por `a_slowed_disk_lowers_health_and_unmeasured_does_not`.

**Degradação de rede fora do score.** A sonda repetida existe (`current_network_score`, [[f02-descoberta-de-rede]]), mas não entra no `health_score`.

**Termo de rede do [[saude-e-confianca]] ausente.** Sem latência de rede medida por nó, não há como
distinguir um nó atrás de um link saturado.

**Um limiar, uma penalidade.** O `HEALTH_WEIGHT` do placement é único: um nó a 100 °C paga
o mesmo fator que um nó com a memória em 95%. Pesos por domínio — térmico pesando mais que
memória — são uma mudança no modelo de custo, não no de saúde, e não têm dado para calibrar.

## Próximos passos

- Baseline térmica por hardware, quando a detecção de hardware (C1) existir para isso.
- SMART/erros de disco e latência de rede, quando houver medida de referência para cada uma.
