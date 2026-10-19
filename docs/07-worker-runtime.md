# 07 — Worker Runtime

## Responsibility

Executar Work Units usando o hardware local mais adequado.

## Components

- Work Unit Engine;
- Memory Manager;
- Tensor Engine;
- Device Manager;
- Local Scheduler;
- Transport Engine;
- Cache Manager;
- Checkpoint Manager;
- Telemetry.

## Hardware backends

Interface abstrata:

```text
Tensor Engine
 |- CPU backend
 |- CUDA/compatible GPU backend
 |- ROCm/compatible backend
 |- Vulkan/OpenCL candidate
 `- NPU backend
```

Começar com CPU x86_64/ARM64.

## Local scheduling

O Global Scheduler decide o node. O Local Scheduler decide CPU cores, GPU/NPU e buffers.

## Priority

CRITICAL, INTERACTIVE, NORMAL, BACKGROUND, PREFETCH, CACHE.

## Compute vs transfer

O runtime deve poder decidir entre transferir um tensor e recomputá-lo quando a previsão indicar menor custo total.

## Zero-copy

EXPERIMENTAL: permitir zero-copy/DMA quando o hardware/driver oferecer suporte.
