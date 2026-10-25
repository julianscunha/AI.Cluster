---
title: "Nó e hardware"
aliases: ["Nó e hardware", "docs/04", "04 — Node and Hardware", "04 — Nó e hardware"]
tags: [sistema/subsistema/substrato]
type: sistema/subsistema
status: decidido
implementacao: parcial
fases: ["[[fase-01-nucleo-do-cluster]]", "[[fase-02-capability-e-saude]]"]
---

# Nó e hardware

`Device` é o hardware físico; `Node` é o papel lógico desse hardware no cluster. Separar as duas identidades permite reconhecer o equipamento mesmo quando o nó muda de nome, e impede que um `node_id` reutilizado por outro dispositivo herde a confiança do anterior (ver [[saude-e-confianca]]).

## Identidade

| Conceito | Identifica | Observação |
|---|---|---|
| `Device` | Hardware físico | Sobrevive a reimagem do sistema. Reconhecido por fingerprint de hardware. |
| `Node` | Papel lógico no cluster | Tem estado e confiança próprios, independentes do device. |
| Fingerprint | Equipamento | Usado para separar identidade física de identidade lógica. |

## Descoberta de dispositivos

Coletar, conforme suporte do equipamento:

- arquitetura;
- CPU: modelo, núcleos, frequência e features;
- RAM: capacidade e banda;
- GPU e NPU;
- dispositivos de armazenamento;
- interfaces de rede;
- sensores de temperatura;
- energia, quando disponível.

## Perfil de capacidade

O perfil é multidimensional e carrega três coisas: **baseline** (medido no registro), **current** (remedido depois, com o nó ocioso) e **confidence** (quanto as medidas merecem crédito). Detalhes em [[capability-engine]].

## Classes de hardware

| Classe | Exemplo |
|---|---|
| x86_64 CPU | Servidor ou PC comum |
| x86_64 + GPU | Estação com acelerador |
| ARM64 CPU | Raspberry Pi e similares |
| ARM64 + GPU | SBC com GPU |
| ARM64 + NPU | SBC ou mobile com NPU |
| SBC | Placa de uma peça |
| Mobile | Celular ou tablet como worker |

## Abstração heterogênea

Todo equipamento apresenta a mesma interface de Worker. A diferença entre classes aparece no perfil de capacidade e nos backends do [[worker-runtime]], não no protocolo com o controller.

## Desaparecimento de dispositivo

A ausência de heartbeat muda o estado lógico do nó. O controller não depende de desligamento limpo: um nó que some é marcado como offline e seu trabalho volta para a fila ([[hotplug-e-recuperacao]], [[adr-006-hotplug]]).

> [!info] O que já existe
> - Identidade no `NodeIdentity` de `code/proto/cluster.proto`, preenchido em `code/crates/worker/src/main.rs`: `node_id` (variável `AI_CLUSTER_NODE_ID`, senão o hostname), `device_id` (de `/etc/machine-id`), arquitetura (`std::env::consts::ARCH`), versão do SO (`/etc/os-release`) e versão do runtime.
> - Descoberta: CPU (threads, limitadas aos disponíveis no host), memória (`/proc/meminfo`), classes de armazenamento (`/sys/block`), velocidade de rede (`/sys/class/net`) e temperatura (zona térmica do kernel). Override por variável para testes: `AI_CLUSTER_CPU_THREADS`, `AI_CLUSTER_MEMORY_BYTES`, `AI_CLUSTER_STORAGE_CLASSES`, `AI_CLUSTER_NETWORK_SCORE`, `AI_CLUSTER_TEMPERATURE_C`.
> - Desaparecimento: heartbeat a cada 5 s; o controller marca como `OFFLINE` o nó sem batida por `AI_CLUSTER_HEARTBEAT_GRACE_MS` (padrão 15 000 ms), em `code/crates/controller/src/registry.rs` (`mark_stale_offline`).
> - Fases: [[f01-entrada-e-saida-de-nos]] (registro e saída), [[f02-descoberta-de-rede]], [[f02-benchmark-de-capability]].
>
> Divergência: não há fingerprint de hardware nem `Device` como entidade própria. O `device_id` vem de `/etc/machine-id`, que é a única âncora da identidade física e não é verificada contra o hardware.

> [!todo] Fingerprint de hardware; classes GPU, NPU, SBC e mobile; coleta de energia. Ver [[fase-16-aceleradores]] e [[fase-18-worker-movel]].

> [!warning] Divergência: o campo `accelerators` existe em `CapabilityReport`, mas o worker nunca o preenche. Um nó com GPU aparece como um nó só de CPU.
