# Fase 0 — Validação Experimental

## Objetivo

Provar a premissa física do projeto antes da implementação completa de inferência distribuída:

> adicionar hardware heterogêneo aumenta a capacidade computacional útil de uma única execução?

## Experimentos

1. benchmark de CPU;
2. benchmark de memória;
3. benchmark de armazenamento;
4. benchmark de rede;
5. multiplicação de matrizes local;
6. multiplicação de matrizes distribuída;
7. comparação entre 1, 2 e 3 nodes;
8. falha de worker durante o trabalho.

## Métricas

- tempo de computação;
- tempo de comunicação;
- serialização;
- fila;
- latência;
- throughput;
- speedup;
- eficiência paralela.

## Hipóteses

H1: dividir a computação reduz o tempo quando o ganho de paralelismo superar a comunicação.

H2: adicionar um node pode piorar o desempenho quando comunicação e sincronização dominarem.

H3: nodes heterogêneos precisam receber quantidades diferentes de trabalho.

## Critério de passagem

A fase termina com medição reproduzível de baseline local, execução distribuída, ganho ou perda, custo de comunicação e comportamento após perda de worker.

Nenhuma estratégia definitiva de paralelismo será congelada antes dos resultados.
