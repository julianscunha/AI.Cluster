---
title: "03 — Provisioning"
type: spec
status: draft
tags: [spec, provisioning]
---

# 03 — Provisioning

## Objetivo

Transformar uma máquina com Linux temporário em um node AI Cluster reimaginado.

## Fluxo

```text
Existing Linux
 -> Bootstrap Agent
 -> Controller discovery
 -> Enrollment
 -> Trust validation
 -> Hardware scan
 -> Benchmark
 -> OS image selection
 -> Reimage
 -> Reboot
 -> AI Cluster OS
 -> Post-boot attestation
 -> Runtime benchmark
 -> Model warm-up
 -> READY
```

## Requisito de reimage

O sistema definitivo não deve simplesmente rodar sobre o Linux anterior. O dispositivo deve ser reprovisionado com uma instalação limpa do AI Cluster OS.

## PC/servidor

PROPOSED: PXE/iPXE ou mecanismo equivalente de network boot.

## Móvel

EXPERIMENTAL/OPEN: requer suporte de bootloader, recovery e drivers específicos. O cluster só considera o dispositivo elegível se o caminho de instalação puder ser automatizado e revertido de forma segura.

## Segurança

Antes de apagar o disco:

1. hardware compatibility validada;
2. imagem válida e assinada disponível;
3. rede estável;
4. identidade de provisionamento estabelecida;
5. estratégia de recovery disponível.

## Estados de provisioning

DISCOVERED → ENROLLING → TRUSTED → PROVISIONING → REBOOTING → VALIDATING → BENCHMARKING → READY.
