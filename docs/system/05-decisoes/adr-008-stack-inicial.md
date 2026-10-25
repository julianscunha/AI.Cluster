---
title: "Decisão de stack inicial"
type: operational
status: living
tags: [operational, current-stack]
---

# Decisão de stack inicial

## Proposta atual

- Linguagem: Rust
- RPC: gRPC sobre HTTP/2
- Schema: Protocol Buffers
- CLI: clap
- TUI: ratatui (Fase H)

Estas são escolhas de implementação para o primeiro protótipo, não contratos arquiteturais irreversíveis.

A documentação upstream atual confirma que tonic 0.14.x fornece gRPC sobre HTTP/2 para Rust, prost 0.14.x fornece geração de código/tipos de Protocol Buffers, clap 4.6.x é a linha principal documentada atual, e Ratatui 0.30.x é a linha de release documentada atual no momento desta especificação.
