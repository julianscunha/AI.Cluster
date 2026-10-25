---
title: "State Manager"
aliases: ["Gerenciador de estado", "docs/13", "13 — State Manager"]
tags: [sistema/subsistema/dados]
type: sistema/subsistema
status: proposto
implementacao: parcial
fases: ["[[fase-06-estado-e-recuperacao]]"]
---

# State Manager

O State Manager preserva o estado necessário para retomar uma execução quando o placement ou o hardware mudam. Ele não executa nada: guarda, replica, localiza e restaura. Voltar para [[dados]].

## Classes de estado

| Classe | Conteúdo | Política |
|---|---|---|
| Model State | Pesos e shards | Não é estado de execução. Referenciado pelo [[model-manager]] de forma imutável |
| Execution State | Plano, epoch, sampling, Work Units em voo | Replicado; é o que permite retomar |
| KV Cache | Estado por sequência ([[inference-engine]]) | Seletivo: replica ou checkpoint quando exigido; descartável se reconstruível |
| Checkpoint/Recovery State | Snapshots em pontos seguros | Incremental, conforme política |

## Camadas

```text
HOT  -> RAM / VRAM
WARM -> NVMe / SSD
COLD -> armazenamento persistente ou distribuído
```

A alocação por camada é de [[storage-intelligence]].

## Replicação

Não replicar tudo 1:1. Prioridade: metadados críticos, execution state, KV seletivo e checkpoint conforme política. Caches derivados podem ser descartados.

## Diretório de estado

Referências lógicas, como `state://req-1842/kv/layer-42`. O diretório guarda primary, réplicas e localização. A referência é um endereço: consultá-la não move dado.

## Posse e leases

A posse de um estado usa lease e heartbeat, para que a perda do dono seja detectada rapidamente. Um estado tem um dono por vez; a troca de dono passa pelo lease.

## Checkpoints

Incrementais e em pontos seguros da execução. Não se persiste toda a memória viva a cada token.

## Recuperação

```text
nó perdido
 -> identificar o estado afetado
 -> promover réplica, ou recalcular
 -> restaurar
 -> regenerar placement
 -> continuar (a request permanece a mesma; ver request-manager)
```

## Falhas

- **Perda do dono:** detectada pelo lease; a unidade é reoferecida ou falha se o orçamento de tentativas acabou.
- **Perda do volume do controller:** sem cópia fora dele, a fila se perde junto.
- **Estado corrompido ou ilegível:** nunca restaurar a partir dele; cair para a cópia íntegra ou começar vazio, com alerta.
- **Escrita interrompida:** gravar em arquivo temporário e renomear, para que a versão anterior continue válida.

> [!info] O que já existe (MVP)
> - O controller persiste a **fila de Work Units** em um snapshot (`AI_CLUSTER_STATE`), gravado com arquivo temporário e renomeação, e uma cópia ao lado (`.replica`) no mesmo volume. Sem a variável, fica só em memória.
> - O snapshot guarda só o que ainda não terminou: a fila, o que está em voo com o dono e os resultados que uma unidade ainda vai ler. Resultados sem leitor são descartados.
> - A posse volta ao reload com o mesmo nó, e o resultado em voo é aceito. Depois de um restart, o dono não é reconhecido de imediato; a varredura espera o grace e então reoferece a unidade.
> - Cópia no nó: com `AI_CLUSTER_STATE_REPLICA`, o worker guarda a cópia inteira do snapshot e a devolve no re-registro. O controller adota a cópia só quando não tem trabalho em mãos e ela é mais nova (época em `WorkSnapshot.epoch_unix_ms`).
> - Código: `code/crates/controller/src/work.rs` e `code/crates/worker/src/main.rs`.
>
> Slices: [[f06-persistencia-e-checkpoint]], [[f06-recuperacao]].

> [!warning] Desvio do alvo e migração
> Não há estado por chave. Não existe diretório de estado nem KV cache persistido: o KV cache de uma geração de chat vive na memória do gateway e se perde se o gateway cai. A réplica é a cópia inteira em todo nó configurado, sem fator de replicação escolhido. Migração: estado por chave e KV cache em [[fase-12-model-runtime]].

> [!todo] Falta
> - Diretório de estado com `state://` e localização de réplicas.
> - Persistência só da tabela de leases, ou journal de deltas, em vez de reescrever o conjunto vivo.
> - Fator de replicação e escolha de portador designado.
> - Checkpoint incremental de execução de inferência e KV cache replicado.
> - Persistência do registry e do log de eventos (hoje perdidos no restart).
> - Persistência do lease no momento do despacho (hoje é preguiçosa: pode levar a compute duplicado).
> - Métricas-alvo: idade do último checkpoint, atraso de réplica e tempo de recuperação.
