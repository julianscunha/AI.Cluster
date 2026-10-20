---
title: "Estrutura do repositório"
type: operational
status: living
tags: [operational, repository-structure]
---

# Estrutura do repositório

## Propósito

A implementação é um workspace Rust com fronteiras explícitas de control plane e data plane.

```text
ai-cluster/
├── crates/
│   ├── controller/
│   ├── worker/
│   ├── common/
│   ├── protocol/
│   ├── transport/
│   ├── benchmark/
│   └── cli/
├── proto/
├── os/
├── docs/
├── config/
└── tests/
```

## Regras

- `common` contém apenas tipos de domínio compartilhados.
- `protocol` contém os contratos de rede e os tipos de RPC gerados.
- `controller` é dono do estado do cluster e das decisões de escalonamento.
- `worker` é dono da execução local e da interação com o hardware.
- `transport` abstrai a comunicação física.
- `benchmark` mede capability; não decide placement.
- `cli` é uma interface de operador e nunca executa lógica de worker diretamente.
- Nenhum código de aplicação/agente pertence ao compute plane.
