# Real.AI.Cluster

Runtime distribuído e heterogêneo para computação de IA.

## Objetivo

Transformar equipamentos Linux com capacidades diferentes — servidores, PCs, notebooks, SBCs e dispositivos ARM dedicados — em um pool lógico de recursos capaz de executar cargas de IA de forma distribuída, adaptativa e observável.

O cluster não é um agente de IA. Ele é uma infraestrutura de computação.

## Fronteira do sistema

Consumidores acessam o cluster exclusivamente por API de inferência.

```
Aplicação / Agente / Orquestrador
            │
            ▼
      Inference API
            │
            ▼
       Real.AI.Cluster
            │
      ┌─────┼─────┐
      ▼     ▼     ▼
    Worker Worker Worker
```

Exemplos de consumidores: Hermes, Codex, aplicações próprias e outros orquestradores.

## Princípios

- O plano de aplicação é desacoplado do plano de computação.
- Nodes são heterogêneos, mas executam o mesmo contrato lógico do AI Cluster OS e do Worker Runtime.
- Capacidade é medida por benchmark e telemetria, não inferida apenas pelo nome do hardware.
- Entrada e saída de nodes são operações normais do cluster.
- Estado, modelo e execução são entidades separadas.
- Grafos são usados para conhecimento, topologia, execução e observabilidade.
- A otimização na borda deve reduzir tokens e computação antes de acionar modelos grandes.
- SSD/NVMe é tratado como recurso de cache, staging, estado e swap conforme a saúde e o perfil do dispositivo.
- Toda decisão relevante deve ser observável e explicável.

## Estado da implementação

A arquitetura v0.1 está consolidada. A implementação inicia pela Fase A: Controller, Worker, protocolo, descoberta de capacidade, heartbeat, telemetria e benchmarks distribuídos.

Consulte `docs/` para a arquitetura e `implementation/` para o plano de execução.
