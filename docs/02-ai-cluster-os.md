# 02 — AI Cluster OS

## Objective

Fornecer um Linux mínimo, versionado e orientado a compute, usado de forma uniforme por todos os nodes.

## Requirements

- multi-arch: x86_64 e ARM64;
- boot/recovery;
- network-first;
- worker runtime;
- hardware monitoring;
- storage intelligence;
- secure identity;
- A/B update capability;
- low background resource use.

## Base OS

PROPOSED: usar uma distribuição Linux enxuta/imutável existente como base, em vez de manter uma distribuição do zero.

A escolha final deve considerar kernel support, ARM64, secure boot, driver ecosystem, image build, recovery e lifecycle.

## Runtime layout

```text
/usr/lib/ai-cluster/
  core/
  worker/
  hardware/
  telemetry/
  recovery/

/etc/ai-cluster/
  node.toml
  identity/

/var/lib/ai-cluster/
  cache/
  state/
  models/
  checkpoints/
```

Caminhos são propostos; detalhes finais podem mudar.

## Immutable update

Usar imagens versionadas. A/B deployment é recomendado para updates do sistema, preservando rollback.

## Resource minimization

Desabilitar serviços não essenciais ao compute quando compatível com o hardware: GUI, printing, consumer services, indexadores e demais workloads não necessários.

## Kernel tuning candidates

- CPU governor;
- memory policy;
- huge pages;
- IRQ affinity;
- networking buffers;
- I/O scheduler;
- NUMA awareness quando aplicável;
- thermal policy.

Cada tuning deve ser benchmarked antes de ser adotado globalmente.
