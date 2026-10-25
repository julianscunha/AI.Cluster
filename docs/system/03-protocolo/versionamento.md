---
title: "Versionamento do protocolo"
type: protocol
status: draft
tags: [protocol, versioning]
---

# Versionamento do protocolo

## Regras

- A evolução do schema Protobuf deve ser retrocompatível dentro das janelas de versão suportadas.
- A negociação de features deve ocorrer durante o registro.
- Controller e Worker devem reportar versões de protocolo/runtime separadamente.
- Nós incompatíveis são rejeitados ou colocados em quarentena antes da atribuição de carga.

## Estratégia de upgrade

Prefira upgrades graduais:

```text
node01 update
 -> validate
 -> node02
 -> validate
 -> ...
```

Nunca exija que o cluster inteiro atualize de forma atômica, a menos que seja declarado explicitamente como uma release major de quebra.
