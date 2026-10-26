---
title: "Provisionamento"
aliases: ["docs/03", "03 — Provisioning", "03 — Provisionamento"]
tags: [sistema/subsistema/plataforma]
type: sistema/subsistema
status: proposto
implementacao: nao-iniciado
fases: ["[[fase-15-ai-cluster-os]]", "[[fase-18-worker-movel]]"]
---

# Provisionamento

Provisionamento transforma uma máquina com Linux qualquer em um nó do cluster com o AI Cluster OS
instalado do zero. O objetivo é que a entrada de um dispositivo seja automática, verificada e
reversível: nada é apagado antes de o cluster confirmar que a máquina e a imagem servem.

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

Estados de provisionamento:

```text
DISCOVERED -> ENROLLING -> TRUSTED -> PROVISIONING -> REBOOTING -> VALIDATING -> BENCHMARKING -> READY
```

## Requisito de reimage

O sistema definitivo não roda sobre o Linux que estava na máquina. O dispositivo é reprovisionado
com uma instalação limpa do AI Cluster OS ([[ai-cluster-os]]). Um Linux temporário serve só de
ponte até o reimage.

## Dispositivos de mesa e servidor

Inicialização pela rede: PXE, iPXE ou mecanismo equivalente. Esse caminho ainda é proposta.

## Dispositivos móveis

Em aberto. Exige suporte de bootloader, recuperação e drivers específicos por modelo. Um dispositivo
móvel só é elegível quando o caminho de instalação pode ser automatizado e revertido com segurança.
Ver [[fase-18-worker-movel]].

## Condições antes de apagar o disco

1. Compatibilidade de hardware validada.
2. Imagem válida e assinada disponível.
3. Rede estável.
4. Identidade de provisionamento estabelecida (ver [[seguranca]]).
5. Estratégia de recuperação disponível.

Se qualquer condição falhar, o disco não é tocado e o dispositivo fica em `DISCOVERED` ou
`ENROLLING`.

## Atestação pós-boot

Depois do reboot no AI Cluster OS, o nó prova que a imagem e o worker são os esperados antes de
passar a `VALIDATING`. Sem essa prova ele não recebe trabalho. O conteúdo da atestação está em
[[seguranca]].

## Benchmark e aquecimento

- `BENCHMARKING` mede a capacidade do nó (ver [[capability-engine]]).
- Aquecimento de modelo: carregar as fatias que o nó vai servir antes de `READY`.

> [!info] O que já existe
> - Descoberta de rede e medição de link: [[f02-descoberta-de-rede]].
> - Benchmark de capacidade no registro: [[f02-benchmark-de-capability]].
> - Entrada no cluster (registro, pendente, aprovação), que cobre parte de `ENROLLING` e `TRUSTED`:
>   [[f01-entrada-e-saida-de-nos]] e [[f07-aprovacao-de-no]].
> - Estado `PROVISIONING` no enum de nó, sem caminho que o use: `code/crates/common/src/lib.rs`.
>
> Nada de reimage, bootstrap agent, PXE, assinatura de imagem (só a da release, conferida no nó: [[adr-018-release-assinada-e-medida-do-slot]]), atestação pós-boot (só medida de software) ou aquecimento de
> modelo. A máquina entra no cluster pelo worker rodando no Linux que já tinha.

> [!todo] Tudo que o fluxo acima descreve além da entrada no cluster está pendente, inclusive o
> caminho de instalação móvel. Ver [[fase-15-ai-cluster-os]] e [[fase-18-worker-movel]].

> [!warning] Desvio do alvo: no MVP o nó entra pelo worker rodando no Linux existente. Não há
> reimage, então um nó pode carregar pacotes e configurações que o cluster não controla.
