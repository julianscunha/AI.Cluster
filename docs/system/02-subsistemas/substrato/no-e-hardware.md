---
title: "Nó e hardware"
aliases: ["Nó e hardware", "Node and Hardware"]
tags: [sistema/subsistema/substrato]
type: sistema/subsistema
---

# Nó e hardware

Um nó é um worker registrado no controller. Ele roda em uma máquina física (ou contêiner, em testes) e se apresenta ao cluster com a identidade e a capacidade que mediu ou que lhe foram configuradas.

## Identidade

| Campo | Origem |
|---|---|
| `node_id` | Variável `AI_CLUSTER_NODE_ID`; se ausente, o conteúdo de `/etc/hostname`. |
| `device_id` | `/etc/machine-id`. |
| Arquitetura | Arquitetura da máquina (x86_64 ou ARM64). |
| Versão do sistema | `/etc/os-release`. |
| Versão do runtime | Versão do binário do worker. |

A credencial do nó fica em arquivo local (`AI_CLUSTER_NODE_CREDENTIAL_FILE`). Ela, junto com a aprovação do operador, define a confiança do nó (ver [[saude-e-confianca]]).

## Descoberta

O worker coleta, no registro:

- CPU: número de threads, limitado às threads disponíveis no host;
- memória: `MemTotal` de `/proc/meminfo`;
- armazenamento: classes de disco a partir de `/sys/block` (ver [[storage-intelligence]]);
- rede: maior velocidade negociada entre as interfaces não-loopback, em `/sys/class/net`;
- temperatura: zona térmica do kernel, quando visível.

Para testes e para simular máquinas de tamanhos diferentes no mesmo host, cada valor pode ser fixado:

| Variável | Efeito |
|---|---|
| `AI_CLUSTER_CPU_THREADS` | Threads reportadas (não podem passar do host). |
| `AI_CLUSTER_MEMORY_BYTES` | Memória reportada. |
| `AI_CLUSTER_STORAGE_CLASSES` | Classes de armazenamento reportadas, separadas por vírgula. |
| `AI_CLUSTER_NETWORK_SCORE` | Score de rede fixado. |
| `AI_CLUSTER_TEMPERATURE_C` | Temperatura reportada. |

## Perfil de capacidade

O perfil tem três partes: **baseline** (medido no registro), **corrente** (remedido com o nó ocioso) e **confiança** (quanto as medidas merecem crédito). Detalhes em [[capability-engine]].

## Classes de hardware

Hoje o worker roda em CPU x86_64 e ARM64. Não há detecção de GPU, NPU, SBC ou dispositivo móvel como classes próprias.

> Planejado: fingerprint de hardware para separar `Device` (hardware físico) de `Node` (papel lógico), e classes GPU, NPU, SBC e móvel.

## Entrada e saída

- **Entrada.** O nó se registra com o token de entrada (`AI_CLUSTER_JOIN_TOKEN`) e fica aguardando aprovação do operador, a não ser que o controller tenha `AI_CLUSTER_AUTO_APPROVE` ligado. Sem aprovação, não recebe trabalho.
- **Saída planejada.** Com SIGTERM ou Ctrl+C, o worker para de pedir trabalho, espera os slots em execução devolverem seus resultados (teto de 30 s), para o heartbeat e chama `LeaveNode`. O controller marca o nó como `DRAINING`, devolve à fila as unidades que ainda estiverem em voo e remove o nó do registro.
- **Desaparecimento.** Sem saída limpa, o nó envia heartbeat a cada 5 s. O controller varre os nós a cada 5 s e marca como `OFFLINE` o nó sem heartbeat há mais de `AI_CLUSTER_HEARTBEAT_GRACE_MS` (padrão 15 000 ms). Suas unidades voltam à fila. Quando o nó volta a enviar heartbeat, retorna a `READY`.

## Falhas

- Um worker que morre no meio de uma unidade não avisa o controller. O controller só percebe pelo silêncio do heartbeat.
- Um worker vivo que descarta uma unidade sem executá-la também não avisa. A unidade fica em voo com esse nó até ele sair.

## Garantias

- Um nó só recebe trabalho depois de aprovado e fora dos estados `DRAINING`, `QUARANTINED`, `FAILED` e `OFFLINE`.
- O controller não depende de desligamento limpo para tirar um nó de circulação.

## Limites conhecidos

- A identidade física (`device_id`) vem de `/etc/machine-id` e não é verificada contra o hardware. Quem clona a imagem clona a identidade física.
- O campo de aceleradores existe no relatório de capacidade, mas o worker não o preenche. Um nó com GPU aparece como nó só de CPU.
- Dentro de contêiner, `/sys/block` e as zonas térmicas mostram o host, não o contêiner.
- A grace de heartbeat precisa ficar bem acima do intervalo de 5 s. Um valor curto demais tira do ar um nó saudável.
