---
title: "F18 — Bateria no score de saúde"
aliases: ["Q6", "bateria"]
tags: [implementacao/fase/18, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-18-worker-movel]]"
codigos-antigos: ["Q6"]
sistema: ["[[capability-e-saude]]"]
evidencia: ["code/tests/phase-c-battery.sh"]
---
# F18 — Bateria no score de saúde

- [ ] Superado

## Objetivo

Um nó móvel não pode ser drenado pelo cluster: bateria descarregando perde saúde e, com isso, trabalho.

## O que existe

- O worker lê `/sys/class/power_supply` (tipo `Battery`); `AI_CLUSTER_BATTERY=<percentual>:<charging|discharging>` substitui o hardware nos testes.
- `battery_pressure`: zero na tomada, sem bateria ou acima de 50%; sobe linearmente até 1 em 5%. O score de saúde é limitado por `1 - pressão` (o pior domínio decide).
- `HealthReport` ganhou `battery_level` e `on_battery`; a CLI mostra `bateria=`. Zero sem `on_battery` é "sem bateria visível", não "vazia".
- Evidência: `phase-c-battery.sh` — nó a 15% descarregando tem saúde menor e perde a unidade para o nó saudável.

## Limites

- Só Linux/Android via sysfs; iOS e Android nativo ficam para o app móvel.
- Sem histerese de carga: oscilar em torno de 50% muda o score aos poucos, não em degrau.
