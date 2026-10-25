---
title: "Saúde e confiança"
aliases: ["Saúde e confiança", "Health and Trust", "docs/06", "06 — Health and Trust", "06 — Saúde e confiança"]
tags: [sistema/subsistema/substrato]
type: sistema/subsistema
status: proposto
implementacao: parcial
fases: ["[[fase-02-capability-e-saude]]", "[[fase-07-seguranca-base]]"]
---

# Saúde e confiança

Duas dimensões separadas, que não devem ser confundidas:

- **Saúde** mede a condição operacional do hardware e do runtime: o nó consegue trabalhar agora?
- **Confiança** mede se o nó está autorizado e íntegro: ele pode receber trabalho em princípio?

Um nó pode estar saudável e não confiável (recém-chegado sem aprovação), ou confiável e degradado (temperatura alta). O escalonador usa as duas.

## Saúde

Domínios:

- CPU;
- memória;
- térmico;
- armazenamento;
- rede;
- acelerador.

Estados: `READY`, `ACTIVE`, `DEGRADED`, `DRAINING`, `FAILED`, `OFFLINE`.

## Confiança

Estados: `UNKNOWN`, `UNTRUSTED`, `BOOTSTRAPPED`, `VALIDATED`, `TRUSTED`, `QUARANTINED`, `REVOKED`.

A transição para cada estado de confiança depende de evidência: join token, aprovação do operador, credencial por nó e, na fase de hardening, attestation ([[seguranca]], [[fase-19-hardening-de-producao]]).

## Combinações

Um nó pode estar em combinações como estas:

```text
Health: DEGRADED
Trust: TRUSTED
Capacity: HIGH
State: ACTIVE
```

```text
Health: GREEN
Trust: QUARANTINED
Capacity: HIGH
State: BLOCKED
```

## Diagnóstico automatizado

O sistema registra a causa provável de cada degradação e o impacto estimado sobre o trabalho (por exemplo: "temperatura acima do baseline; previsão de tempo X% maior").

## Térmico

A temperatura é comparada com o baseline específico do hardware, não com um limite genérico. Um mesmo número de °C pode ser normal em um SBC e crítico em um notebook.

## Armazenamento

Detecta aviso de saúde (SMART), desgaste, anomalia de latência e regressão de desempenho, quando o dispositivo expõe esses dados.

> [!info] O que já existe
> - **Saúde** (`HealthReport` em `code/proto/cluster.proto`, montado por `health()` em `code/crates/worker/src/main.rs`): `health_score` (0..1), utilização de CPU, memória e swap, temperatura, operações de disco por segundo. Enviado a cada batida (5 s) pelo `ReportHealth` (`HealthStreamRequest`), junto com o perfil de capacidade corrente.
> - Pontuação: o pior domínio decide. Pressão térmica sobe a partir de 80 °C e satura em 100 °C; um nó medido nunca reporta exatamente 0 (piso de 0.05), porque 0 significa "não reportado" para o placement. Um `health_score` baixo entra no placement como penalidade (`HEALTH_WEIGHT`, `code/crates/controller/src/placement.rs`).
> - **Estados**: `NodeState` em `code/crates/common/src/lib.rs` tem os 11 valores do registro, mas só `READY` (registro), `OFFLINE` (sem batida na janela de `AI_CLUSTER_HEARTBEAT_GRACE_MS`) e `DRAINING` (saída planejada) são atribuídos pelo controller. `DEGRADED`, `ACTIVE`, `FAILED` e `QUARANTINED` existem como valores, sem transição que os produza.
> - **Elegibilidade** (`eligible` em `code/crates/controller/src/registry.rs`): recebe trabalho o nó com `trusted` e fora de `DRAINING`, `QUARANTINED`, `FAILED` e `OFFLINE`.
> - **Confiança** hoje é um booleano `trusted` (`NodeSnapshot`): um nó novo nasce não confiável e só recebe trabalho após `ApproveNode`; `RevokeNode` o tira de circulação. A confiança persiste com a credencial do nó em arquivo (`AI_CLUSTER_NODE_CREDENTIAL_FILE` no worker, registro no controller).
> - Fases: [[f02-modelo-de-saude]], [[f07-aprovacao-de-no]], [[f07-token-de-join]], [[f07-operador-e-credencial-por-no]], [[f07-tls]].

> - **Quarentena** (`QUARANTINED`): o controller confere cada resultado de Work Unit (forma, números finitos, checksum); 3 inválidos seguidos tiram o nó de circulação, devolvem suas unidades à fila e registram `node_quarantined`. `approve` libera. Ver [[f04-quarentena]].

> [!todo] Estados de confiança `UNKNOWN`, `BOOTSTRAPPED`, `VALIDATED` e `REVOKED` como máquina de estados (hoje `trusted` sim/não, mais `QUARANTINED`); transições de `DEGRADED`, `ACTIVE`, `FAILED` e `QUARANTINED`; attestation ([[fase-19-hardening-de-producao]]); diagnóstico automatizado; limite térmico por hardware; saúde de armazenamento (SMART) e de rede.

> [!warning] Divergência: `HealthReport` declara `storage_health`, `network_latency_ms` e `thermal_throttling`, mas o worker nunca os preenche (ficam em zero). O CLI os omite de propósito para não exibir zero como medida. O limite térmico é genérico (80 °C e 100 °C), não o baseline do hardware.