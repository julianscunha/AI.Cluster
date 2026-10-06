# Gerenciador de Estado

## Objetivo

Manter o estado necessário para recuperar e continuar execuções quando a topologia do cluster mudar.

## Classes

```
Model State
Execution State
KV Cache
Checkpoint / Recovery
```

## Hierarquia

```
HOT  → RAM / VRAM / memória de acelerador
WARM → NVMe / SSD
COLD → armazenamento remoto
```

## Diretório de estado

Cada objeto possui identidade, versão, localização primária e, quando aplicável, réplica.

## Réplica seletiva

Estado crítico recebe proteção maior. Caches descartáveis podem existir sem réplica.

## Leases

Nodes que mantêm estado ativo renovam leases. Expiração permite recuperação por outro node.

## KV Cache

O KV Cache é particionável e possui metadata de camada, sequência, precisão, tamanho, owner e réplica.

## Checkpoint

Checkpoints não precisam ser criados a cada token. O intervalo deve ser configurável e adaptativo.

## Recuperação

```
node perdido
→ detectar
→ localizar estado afetado
→ promover réplica ou reconstruir
→ gerar novo plano
→ continuar
```

## Plano de execução

O estado referencia a versão do Execution Plan que o produziu. Isso evita inconsistência quando o placement muda.

## Persistência

Metadados críticos têm persistência confiável. Buffers temporários podem permanecer apenas em memória ou cache.

## Integridade

Objetos persistidos possuem versão e checksum quando aplicável.

## Hot-plug

A entrada de um novo node pode ser usada para reconstruir réplicas e recuperar capacidade perdida.

## Critério

O desaparecimento de um worker não deve exigir reinício global do Controller ou das requisições não afetadas.
