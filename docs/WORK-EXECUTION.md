# Execução de Work Unit

## Objetivo

Validar o primeiro caminho completo de trabalho:

```text
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

## Seleção de Worker

O Controller mantém a carga ativa de cada Worker e calcula um score para selecionar candidatos.

A seleção não usa mais o timestamp do heartbeat como critério de capacidade. O score inicial considera capacidade de compute, saúde, carga ativa e RTT de rede.

A reserva de capacidade é criada antes da execução e liberada ao final, inclusive em caso de falha.

## Multiplicação matricial distribuída

A primeira operação realmente distribuída divide as linhas da matriz de saída entre Workers elegíveis.

```text
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

A matriz A já é particionada por linhas: cada Worker recebe somente seu shard de A. A matriz B ainda é replicada integralmente para cada Worker.

A próxima otimização será distribuir B por shards quando o benefício de reduzir tráfego superar o custo de sincronização, cache e acesso aos blocos compartilhados.

## CLI

Execução de um benchmark com um limite de Workers:

```text
ai-cluster benchmark distributed-matmul --rows 128 --inner 128 --cols 128 --max-workers 2
```

O comando retorna Workers utilizados, duração total, duração crítica de compute, overhead aproximado, bytes de entrada enviados, tamanho do resultado e checksum.

Para medir escalabilidade automaticamente:

```text
ai-cluster benchmark scaling --rows 256 --inner 256 --cols 256 --worker-counts 1,2,3
```

O comando executa sequencialmente os pontos solicitados, garante a presença do baseline de 1 Worker, valida o checksum entre execuções e calcula speedup e eficiência paralela.

A duração total inclui comunicação, serialização e demais overheads observados pelo Controller. `compute_duration_us` representa o maior tempo de compute retornado entre os Workers, portanto é a parcela crítica do trabalho paralelo.
