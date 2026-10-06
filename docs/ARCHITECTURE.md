# Arquitetura

## Visão geral

O Real.AI.Cluster é uma plataforma de execução distribuída para IA. Sua função é transformar recursos heterogêneos em capacidade computacional coordenada, sem expor a topologia interna aos consumidores.

```
                    APPLICATION PLANE
         Hermes / Codex / Aplicações / Agentes
                         │
                     Inference API
                         │
                         ▼
                    CONTROLLER
                         │
      ┌──────────────────┼──────────────────┐
      ▼                  ▼                  ▼
 Edge Intelligence  Execution Planner   State Manager
      │                  │                  │
      └──────────────────┼──────────────────┘
                         ▼
                     Scheduler
                         │
               ┌─────────┼─────────┐
               ▼         ▼         ▼
            Worker    Worker    Worker
               │         │         │
            CPU/GPU    CPU/NPU    CPU/GPU
               └─────────┼─────────┘
                         ▼
                     Telemetry
                         │
                         ▼
                 Performance Model
```

## Componentes do Controller

### Cluster Manager
Mantém membership, identidade lógica dos nodes e estado operacional.

### Provisioning Manager
Coordena o ingresso, reprovisionamento bare-metal e instalação do AI Cluster OS.

### Security & Trust
Controla enrollment, autenticação, confiança, quarentena e revogação.

### Edge Optimizer
Executa cache, resolução por grafo, redução de contexto, deduplicação, seleção de ferramentas e outras otimizações de baixo custo.

### Graph Engine
Mantém projeções de relações de conhecimento, topologia, execução, dependências e observabilidade.

### Model Manager
Gerencia modelos, variantes, quantizações, shards, cache e placement.

### State Manager
Gerencia estado de execução, KV cache, checkpoints e recuperação.

### Execution Planner
Transforma o Model Graph em um Execution Graph e produz planos de execução.

### Scheduler
Escolhe placement e ordem das Work Units considerando capacidade, localidade, rede, saúde, temperatura, confiança e custo.

### Observability
Consolida métricas, eventos, traces, diagnóstico e histórico.

## Camadas do node

```
AI Cluster OS
├── Kernel / Drivers
├── Hardware Abstraction Layer
├── Cluster Agent
├── Worker Runtime
├── Storage Intelligence
├── Health Monitor
└── Telemetry Agent
```

O mesmo contrato lógico deve funcionar em x86_64 e ARM64, com backends específicos para CPU, GPU e NPU.

## Regra de desacoplamento

O consumidor nunca conhece:

- shards;
- nodes;
- transporte;
- KV cache;
- placement;
- scheduler;
- topologia.

Esses detalhes são internos ao Compute Plane.

## Requisitos arquiteturais

O sistema deve suportar:

1. múltiplos nodes heterogêneos;
2. hot-plug e drain;
3. reprovisionamento automático;
4. execução de uma única requisição em múltiplos recursos;
5. replanejamento durante a execução em pontos seguros;
6. cache e memória hierárquica;
7. observabilidade detalhada;
8. otimização de tokens e computação na borda;
9. recuperação de falhas;
10. exposição de uma API externa estável.
