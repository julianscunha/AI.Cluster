---
title: "Provisionamento"
aliases: ["Provisionamento", "Provisioning"]
tags: [sistema/subsistema/plataforma]
type: sistema/subsistema
---

# Provisionamento

Provisionamento é o caminho de uma máquina até ser um nó do cluster. Hoje ele é feito pelo operador, com três caminhos e aprovação manual. O objetivo de longo prazo é que a entrada seja automática, verificada e reversível: nada é apagado antes de o cluster confirmar que a máquina e a imagem servem. Voltar para [[plataforma]].

## Caminhos de entrada

| Caminho | Quando usar | Como |
|---|---|---|
| Worker sobre Linux existente | Máquina com Linux já instalado, com systemd ou sem | `os/bootstrap.sh` como root. Ver [[ai-cluster-os]]. |
| Imagem em cartão | Placa armv7 com o AI Cluster OS | `install-disk.py` grava a imagem a partir de um ambiente em memória. |
| Imagem em disco x86_64 | PC, notebook ou servidor UEFI | `x86/install-disk.sh` grava a imagem GPT (ESP, slots A/B, dados) a partir de qualquer Linux; apaga o disco inteiro. |
| Boot pela rede | Placa armv7 que já tem um sistema qualquer | `install-netboot.sh`, com retorno ao cartão. |

Em qualquer caminho, o worker chama o controller com o token de join e recebe uma credencial. A máquina entra pendente, e o operador aprova com `aic cluster node approve`. Ver [[seguranca]].

## Condições antes de gravar o disco

O instalador de cartão só grava quando:

1. a raiz do ambiente é o ambiente de instalação esperado;
2. o cartão está desmontado;
3. a execução recebeu `--yes`.

Se qualquer condição falha, o cartão não é tocado.

## Após a entrada

1. O worker mede o hardware e informa capacidade ao controller ([[capability-engine]]).
2. O operador aprova o nó.
3. O nó passa a receber trabalho e aparece como `READY` em `aic cluster nodes`.

Sem reinstalação, a máquina continua com o Linux que já tinha, e pacotes e configurações fora do controle do cluster continuam lá.

> Planejado: fluxo completo de provisionamento, com descoberta automática do dispositivo, um agente de bootstrap, verificação de hardware antes de qualquer alteração, escolha da imagem pelo cluster, reinstalação disparada pelo controller e atestação pós-boot antes de liberar trabalho.
> Planejado: estados de provisionamento visíveis na CLI: descoberto, registrando, confiável, provisionando, reiniciando, validando, em benchmark e pronto.
> Planejado: aquecimento de modelo antes de o nó ficar pronto, carregando as fatias que ele vai servir.
> Planejado: boot por rede para servidores e estações, por PXE ou iPXE.
> Planejado: caminho de instalação para dispositivos móveis, com bootloader e recuperação próprios por modelo. Um dispositivo móvel só entra no provisionamento quando a instalação puder ser automatizada e revertida.

## Limites conhecidos

- **Aprovação manual de cada nó.** Não há enrollment automático.
- **Sem reinstalação pelo cluster.** Reinstalar um nó é trabalho do operador, com cartão ou boot de rede.
- **Imagem para armv7 e x86_64.** ARM64 entra pelo caminho de bootstrap, que depende do Linux já instalado. A imagem x86_64 foi validada só em QEMU.
- **Entrada sem verificação de origem.** O binário do worker usado no bootstrap não é verificado por assinatura, e o token de join fica em claro no arquivo de ambiente do worker.
- **Sem medição de hardware antes de alterar a máquina.** O operador decide se a placa serve.
