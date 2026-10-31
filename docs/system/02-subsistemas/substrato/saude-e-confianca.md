---
title: "Saúde e confiança"
aliases: ["Saúde e confiança", "Health and Trust"]
tags: [sistema/subsistema/substrato]
type: sistema/subsistema
---

# Saúde e confiança

Duas dimensões separadas:

- **Saúde** responde se o worker consegue trabalhar agora.
- **Confiança** responde se o worker está autorizado e íntegro, ou seja, se pode receber trabalho em princípio.

Um worker pode estar saudável e não confiável (recém-chegado, ainda sem aprovação), ou confiável e degradado (temperatura alta). O escalonador usa as duas.

## Saúde

O relatório de saúde chega a cada heartbeat (5 s). Ele tem um escore entre 0 e 1 e os valores brutos: utilização de CPU, memória e swap, temperatura, operações de disco por segundo e latência de rede.

O escore é o **pior domínio**, não a média:

| Domínio | Medida | Fonte |
|---|---|---|
| Pressão de memória | `1 − MemAvailable / MemTotal` | `/proc/meminfo` |
| Pressão de swap | `1 − SwapFree / SwapTotal` | `/proc/meminfo` |
| Pressão térmica | Rampa de 80 °C (0) a 100 °C (1) | Zona térmica do kernel ou `AI_CLUSTER_TEMPERATURE_C` |
| Saúde de armazenamento | Taxa de escrita com `fsync` atual sobre o baseline; razão ≥ 0,8 conta como 1,0 | Sonda remedida no ocioso ou `AI_CLUSTER_STORAGE_HEALTH` |

Regras:

- **CPU não entra no escore.** Utilização mede carga, não condição. A carga é tratada pelo termo de fila do custo de placement.
- **Piso de 0,05.** Um worker medido nunca reporta exatamente 0, porque 0 significa "não reportado" para o placement.
- **Sem sensor não penaliza.** Temperatura zero significa que nenhum sensor está visível, e o domínio é ignorado.
- O escore entra no custo de placement como fator multiplicativo (peso 1,0). Um worker degradado perde para um saudável de capacidade igual.

Os limites de temperatura são genéricos (80 °C e 100 °C), iguais para qualquer hardware.

## Confiança

O controller guarda a confiança de cada worker como aprovado ou não aprovado, com uma exceção de quarentena:

| Situação | Efeito |
|---|---|
| Worker novo | Não confiável. Não recebe trabalho até ser aprovado (`aic cluster node approve`). |
| Aprovado | Recebe trabalho. |
| Revogado (`RevokeNode`) | Sai de circulação. Suas unidades em voo voltam à fila. |
| Quarentena | Sai de circulação até um operador aprovar de novo. Ver abaixo. |

A aprovação e a credencial do worker sobrevivem a reinício quando o controller tem `AI_CLUSTER_NODES` configurado. Registrar o worker de novo não o cura, e sair do cluster não apaga o registro: o worker continua listado, como `QUARANTINED` se estava em quarentena.

Toda RPC exige o token de entrada, e a credencial do worker é conferida em cada chamada. O papel de operador é separado do papel de worker (ver [[seguranca]]).

## Quarentena

Um resultado é inválido quando não tem a forma esperada, tem números não finitos ou tem checksum diferente do que a soma recomputada pelo controller daria (mesma ordem de soma, igualdade exata).

- Um resultado inválido não é aceito. A unidade volta à fila **sem gastar** a tentativa dela, porque a culpa é do worker.
- Três resultados inválidos seguidos colocam o worker em quarentena. Um resultado válido zera a contagem.
- Ao entrar em quarentena, o worker perde as unidades que ainda segura (cada uma volta à fila gastando uma tentativa) e os canais de fatias são derrubados.
- Só `aic cluster node approve` libera o worker. O limite de três é fixo, sem variável de ambiente.

## Diagnóstico

O sistema não registra a causa provável de uma degradação nem o impacto estimado sobre o trabalho. O operador vê os valores brutos do relatório de saúde e a coluna de saúde em `aic cluster nodes`.

## Limites conhecidos

- O checksum prova coerência interna do resultado, não que a multiplicação está certa. Um worker que devolve uma matriz errada com o checksum dessa matriz passa pela validação.
- Não há verificação por réplica. Executar a mesma unidade em dois workers e comparar custaria o dobro de compute.
- Não há SMART, medida de desgaste de disco nem limite térmico por hardware.
- A degradação de rede não entra no escore de saúde. O custo de transferência usa a medida corrente, mas o escore não.
- Não há estados `UNKNOWN`, `BOOTSTRAPPED` ou `VALIDATED`. A confiança é binária (aprovado ou não) mais a quarentena.
- Os estados `DEGRADED`, `ACTIVE` e `FAILED` existem no enum, mas o controller não os produz. O estado de um worker que parou de responder é `OFFLINE`.

> Planejado: níveis de confiança com provisionamento e verificação (`UNKNOWN` até `TRUSTED`), attestation, limite térmico por hardware, SMART e diagnóstico automatizado com impacto estimado no trabalho.
