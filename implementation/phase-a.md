# Fase A — Fundação

## Objetivo

Criar a primeira versão executável do Controller, Worker, protocolo e CLI.

## Componentes

```
controller
worker
protocol
transport
benchmark
common
cli
```

## Fluxo mínimo

```
Worker
→ RegisterNode
→ Controller
→ Heartbeat
→ Health
→ Capability
→ CLI
```

## Tecnologia inicial

Rust, Protocol Buffers e gRPC para o protocolo de controle. O desenho separa transporte e execução para permitir novos meios posteriormente.

## Entidades

- Node;
- Device;
- Capability;
- Health;
- Work Unit;
- Event.

## Critérios de aceite

Com dois Linux na mesma rede:

1. ambos se registram;
2. ambos enviam heartbeat;
3. o Controller apresenta o estado;
4. a perda de um node é detectada;
5. a CLI lista os nodes;
6. capacidades básicas são identificadas.

## Fora do escopo

LLM real, GPU/NPU, paralelismo de tensor, hot-plug de inferência, provisioning bare-metal e API pública.
