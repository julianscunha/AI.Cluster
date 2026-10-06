# Execução de Work Unit

## Objetivo

Validar o primeiro caminho completo de trabalho:

```
Cliente
→ Controller
→ seleção de Worker
→ Worker Runtime
→ execução
→ resultado
```

## Work Unit básica

A primeira implementação possui identificador, operação, payload e prioridade.

Operações iniciais:

- `echo`: retorna o payload;
- `cpu_spin`: executa iterações de CPU.

## Multiplicação matricial distribuída

A primeira operação realmente distribuída divide as linhas da matriz de saída entre Workers elegíveis.

```
Matriz A × Matriz B
       │
       ▼
Controller
       │
 ┌─────┼─────┐
 ▼     ▼     ▼
W1    W2    W3
linhas 0..  ...
       │
       ▼
recomposição da matriz C
```

Cada Worker recebe as matrizes e calcula somente seu bloco de linhas. O Controller executa os blocos em paralelo e recompõe o resultado na ordem correta.

## Objetivo do benchmark

Comparar:

- execução local;
- execução com dois Workers;
- execução com mais Workers;
- tempo de computação;
- custo de comunicação;
- speedup;
- eficiência paralela.

## Limitação atual

O particionamento ainda é simples: as matrizes completas são enviadas a cada Worker. Isso é proposital para o primeiro experimento.

A otimização de distribuição de tensores, cache, compressão, locality e transferência incremental será implementada depois que o comportamento básico for medido.

## CLI

```
ai-cluster benchmark distributed-matmul --rows 128 --inner 128 --cols 128
```

O comando retorna Workers utilizados, duração, tamanho do resultado e checksum.
