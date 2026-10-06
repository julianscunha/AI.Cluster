# Descoberta de Hardware

## Objetivo

Construir um perfil observável do equipamento sem depender do nome comercial do dispositivo.

## Primeira etapa

Em Linux, a Fase A coleta por interfaces do sistema:

- arquitetura;
- quantidade de CPUs lógicas;
- modelo textual do processador quando disponível;
- memória total;
- dispositivos de bloco;
- tamanho do dispositivo;
- indicação rotacional;
- classificação inicial em HDD, SSD SATA ou NVMe.

## Classificação de armazenamento

A classificação usa a indicação de rotação do kernel e o nome do dispositivo. NVMe é identificado pelo prefixo do dispositivo e discos rotacionais são classificados como HDD.

Essa classificação é inicial. A Fase B deve adicionar throughput, latência, IOPS, temperatura, SMART/NVMe health e endurance.

## Princípio

Hardware identificado não significa hardware já medido. Discovery produz fatos do dispositivo; Benchmark produz capacidade observada.

## Evolução

O perfil final deverá alimentar:

- Scheduler;
- Storage Intelligence;
- Health Engine;
- Model Manager;
- Performance Model.
