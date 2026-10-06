# Engine de Inferência

## Objetivo

Executar modelos Transformer sobre o Execution Graph distribuído.

## Fases

```
Prefill
→ preparação do contexto e construção inicial do estado

Decode
→ geração autoregressiva
```

## Geração autoregressiva

Cada token depende do estado acumulado. Paralelismo de uma única requisição ocorre principalmente dentro das operações e entre componentes independentes, não por simples divisão de tokens futuros.

## KV Cache

O KV Cache é particionável e gerenciado pelo State Manager. O Engine trabalha com referências ao estado.

## Paralelismo

O Engine deve permitir paralelismo por pipeline, por tensor, híbrido, paralelismo de kernels e sobreposição entre comunicação e computação.

## Precisão

A execução deve respeitar a precisão do modelo e as capabilities do backend. Conversões devem ser explícitas e mensuradas.

## Decodificação especulativa

Quando habilitada, um modelo menor produz candidatos e o modelo principal verifica múltiplos tokens.

## Recuperação

A execução possui pontos seguros de checkpoint. Falha de node provoca recuperação de estado e novo placement.

## Métricas

```
TTFT
ITL
tokens por segundo
tempo de prefill
tempo de decode
tempo de kernel
tempo de comunicação
uso de KV Cache
tempo de recuperação
```

## Critério

O Engine é independente do agente consumidor e utiliza o Worker Runtime para execução física.
