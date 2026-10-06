# AI Cluster OS

## Objetivo

Criar um Linux dedicado ao cluster, instalado em bare metal após o equipamento ser adotado.

O Linux original do equipamento é apenas bootstrap e pode ser removido durante o provisionamento.

## Fluxo

```
Linux existente
→ Bootstrap
→ Controller
→ validação
→ reimagem
→ AI Cluster OS
→ Worker Runtime
→ JOIN definitivo
```

## Características

- Linux mínimo;
- imagem imutável;
- multi-arquitetura;
- ausência de desktop e serviços desnecessários;
- recuperação automática;
- monitoramento de hardware;
- atualização controlada.

## Provisionamento

A imagem deve ser escolhida a partir de:

- arquitetura;
- CPU;
- GPU;
- NPU;
- drivers;
- armazenamento;
- necessidades do dispositivo.

## Reimagem

O provisionamento inicial realiza instalação limpa no hardware aprovado.

## Atualização

Depois de instalado, o OS deve usar atualização A/B ou mecanismo equivalente para reduzir risco de falha.

## Boot

```
Firmware
→ Bootloader
→ Kernel
→ Hardware Discovery
→ Cluster Agent
→ Validation
→ Worker Ready
```

## Hardware Abstraction Layer

A HAL separa o runtime lógico das diferenças entre CPU, GPU, NPU e demais aceleradores.

## Storage Intelligence

O OS identifica HDD, SATA SSD, NVMe e características de saúde, velocidade e endurance.

Políticas de cache, staging, estado e swap são adaptadas ao perfil do dispositivo.

SSD/NVMe saudável e rápido pode receber uso agressivo de cache e staging; HDD recebe políticas conservadoras.

## ARM64 e x86_64

O contrato do cluster é comum às duas arquiteturas. Imagens, kernel e backends podem diferir.

## Recovery

O node deve ser capaz de reiniciar, validar o estado e retornar ao cluster quando possível.
