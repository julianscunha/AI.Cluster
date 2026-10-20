---
title: "Estratégia de testes"
type: implementation
status: proposed
tags: [implementation, testing-strategy]
---

# Estratégia de testes

## Camadas de teste

1. Testes unitários.
2. Testes de compatibilidade de protocolo.
3. Testes de integração.
4. Injeção de falhas.
5. Benchmarks de desempenho.
6. Testes de soak.
7. Testes de recuperação.

## Cenários obrigatórios

### Entrada de nó

Nó novo entra, recebe configuração e benchmark.

### Drenagem de nó

Nó drena enquanto há requisições ativas.

### Falha brusca de nó

Mate o worker/processo ou desligue o nó e verifique a recuperação.

### Degradação térmica

Reduza artificialmente a capability do nó e verifique se o scheduler muda o peso.

### Degradação de rede

Injete latência/redução de banda e verifique o replanejamento ciente do transporte.

### Degradação de armazenamento

Injete latência de disco elevada e verifique se a política de storage muda.

### Acurácia de predição

Compare latência de execução e throughput previstos vs reais.

### Otimização de tokens

Meça:

- tokens antes;
- tokens depois;
- tokens evitados;
- latência do otimizador;
- latência ponta a ponta.

## Métricas de escala

Nunca alegue escala linear sem medição. Compare:

```text
N nós
vs
throughput
vs
latência de requisição única
vs
overhead de comunicação
```

## Matriz de benchmark

No mínimo:

- mesmo hardware / mesma rede;
- hardware heterogêneo / mesma rede;
- mesmo hardware / rede degradada;
- só CPU;
- CPU + acelerador;
- só RAM vs assistido por SSD;
- cache habilitado vs desabilitado;
- recuperação por grafo vs recuperação base.
