# Descoberta de rede (Fase C1)

O perfil de capability carrega `network_score` desde o protocolo inicial, e o modelo de
placement lê o campo desde G2 — mas nada o preenchia. Esta fatia fecha isso: o worker lê a
velocidade do próprio link e reporta. É a metade "bandwidth" de `docs/05`; a metade "latency"
fica para a medição ativa.

## O que é medido

`/sys/class/net/<iface>/speed` — a velocidade **negociada** do link, em Mbps. O worker pega a
maior entre as interfaces não-loopback, que é a que uma transferência usaria. Um veth de
contêiner, uma bridge ou um membro de bond não têm taxa negociada e reportam erro ou valor
negativo; esses são pulados, não lidos como "sem link".

## A escala é relativa, como a do compute

`network_score` não é bytes por microssegundo: é a velocidade do link **sobre a referência de
1 GbE** (`REFERENCE_LINK_MBPS`). A mesma forma do `compute_score`, medido sobre o
`REFERENCE_CORE_GFLOPS`:

| link | `network_score` |
| --- | --- |
| 100 MbE | 0.1 |
| 1 GbE | 1.0 |
| 2.5 GbE | 2.5 |
| 10 GbE | 10.0 |

Isso é de propósito. O termo de transferência do modelo de custo divide os bytes da unidade
pelo `network_score`, e caía em 1.0 quando não medido. Aquele 1.0 já era, implicitamente, "um
link de referência" — a normalização só torna explícito o que o fallback já assumia. Sem ela,
preencher o campo com bytes/µs de verdade (10 GbE = 1250) encolheria o termo 1000× e mataria o
componente de comunicação do custo, que passaria a ficar sempre abaixo da banda de histerese e
nunca decidiria nada.

Um nó que não reporta taxa é tratado como estando no link de referência, não como sem rede — a
mesma convenção de `memory_bytes == 0` pular o filtro de memória em vez de ler "sem RAM".

## Botão de determinismo

`AI_CLUSTER_NETWORK_SCORE` fixa o número, sem ler interface — o mesmo contrato de
`AI_CLUSTER_COMPUTE_SCORE`. É o que permite pôr um nó de link lento ao lado de um de link
rápido numa única máquina, que é como o E2E prova o consumidor.

## Evidência

`bash code/tests/phase-c-network.sh`:

- A) nó real: `rede_score=10.00` — o host é 10 GbE, então o número é a leitura do link sobre a
  referência;
- B) `AI_CLUSTER_NETWORK_SCORE=4.0` → `rede_score=4.00`, o pin manda no número;
- C) baseline primeiro-a-chegar, só o nó de link lento: a unidade vai para `node-slow`;
- D) com placement e os dois nós ociosos com o **mesmo** compute, o custo manda a unidade para
  `node-fast` — o termo de transferência decide, não a ordem de chegada.

A perna C é o que dá sentido à D: sem ela, a unidade ir para o nó de link rápido poderia ser
acaso, não custo.

## Teto conhecido

**`network_score` é a velocidade nominal**: o que o link *é*, não o que ele *faz* sob carga.
A taxa alcançada é a sonda do C2, abaixo.

**Latência não é medida.** `network_latency_ms` continua sem preenchimento e sem consumidor.
Só passa a valer quando houver um segundo transporte ou uma política que a use.

**Aceleradores não.** GPU/NPU não são detectados porque não há caminho de execução para eles:
reportá-los seria anunciar hardware que o worker não usa. É a fase J.

## Taxa alcançada (Fase C2)

No registro o worker chama `ProbeLink`: sobe 3 payloads de 2 MiB ao controller, cronometra cada
RPC e fica com a **melhor** corrida (contenção só atrasa, então a mais rápida é a capacidade
real do link, o mesmo raciocínio do benchmark de compute). O resultado vai em
`network_achieved_score`, na mesma escala (1.0 = 1 GbE), e o detalhe do nó mostra
`rede_alcancada=`. O placement usa o **menor** entre o nominal e o alcançado: o link é o teto,
a medida é o que ele entregou. Zero em qualquer um é "não medido" e sai da conta. Um nó com
`AI_CLUSTER_NETWORK_SCORE` fixado não sonda (`rede_alcancada=-`).

Evidência: `phase-c-network.sh` A) nó real `rede_score=10.00 rede_alcancada=2.74`; B) pin,
`rede_alcancada=-`.

Teto: só upload, e só até o controller; medido uma vez, no registro, não remedido como o
compute (C3). Worker e controller no mesmo host medem loopback; o teto nominal contém isso.
