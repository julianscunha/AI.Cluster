---
title: "04 — Node and Hardware"
type: spec
status: draft
tags: [spec, node-and-hardware]
---

# 04 — Nó e hardware

## Nó vs Device

`Device` identifica hardware físico. `Node` identifica o papel lógico desse hardware no cluster.

## Descoberta de dispositivos

Coletar, conforme suporte:

- architecture;
- CPU model/cores/frequency/features;
- RAM/capacity/bandwidth;
- GPU/NPU;
- storage devices;
- network interfaces;
- temperature sensors;
- power data quando disponível.

## Fingerprint de hardware

Usado para reconhecer o equipamento e separar identidade física de identidade lógica.

## Perfil de capacidade

Deve ser multidimensional e conter baseline, current e confidence.

## Classes de hardware

- x86_64 CPU;
- x86_64 + GPU;
- ARM64 CPU;
- ARM64 + GPU;
- ARM64 + NPU;
- SBC;
- mobile device.

## Abstração heterogênea

Todos os equipamentos devem apresentar uma interface de Worker comum.

## Desaparecimento de dispositivo

A ausência de heartbeat muda o estado lógico; o Controller não depende de desligamento limpo.
