# 04 — Node and Hardware

## Node vs Device

`Device` identifica hardware físico. `Node` identifica o papel lógico desse hardware no cluster.

## Device discovery

Coletar, conforme suporte:

- architecture;
- CPU model/cores/frequency/features;
- RAM/capacity/bandwidth;
- GPU/NPU;
- storage devices;
- network interfaces;
- temperature sensors;
- power data quando disponível.

## Hardware fingerprint

Usado para reconhecer o equipamento e separar identidade física de identidade lógica.

## Capability profile

Deve ser multidimensional e conter baseline, current e confidence.

## Hardware classes

- x86_64 CPU;
- x86_64 + GPU;
- ARM64 CPU;
- ARM64 + GPU;
- ARM64 + NPU;
- SBC;
- mobile device.

## Heterogeneous abstraction

Todos os equipamentos devem apresentar uma interface de Worker comum.

## Device disappearance

A ausência de heartbeat muda o estado lógico; o Controller não depende de desligamento limpo.
