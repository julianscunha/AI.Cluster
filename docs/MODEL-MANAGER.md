# Gerenciador de Modelos

## Objetivo

Controlar modelos, versões, variantes de precisão, shards, compatibilidade, cache e distribuição física.

## Registro

```
Modelo
→ Versão
→ Variante
→ Artefatos
→ Shards
```

## Manifesto

Cada modelo deve registrar arquitetura, parâmetros, contexto, tokenizer, formato, checksum e capacidades de execução.

## Variantes

O mesmo modelo pode ter FP16, FP8, INT8, Q8, Q6, Q5, Q4 e outras representações.

## Shards

Shards são artefatos independentes e versionados. Placement é mantido separadamente da identidade do shard.

## Compatibilidade

Cada variante declara arquitetura, memória mínima, precisão e aceleradores compatíveis.

## Cache

Nodes podem manter cópias quentes em RAM e cópias mornas em NVMe/SSD.

## Prefetch

O gerenciador pode carregar shards antes de serem necessários quando o plano tiver confiança suficiente.

## Aquecimento

Novo node passa por carregamento e warm-up antes de receber workload crítico.

## Replicação

Replication factor é configurável por criticidade. O cluster não precisa replicar todos os shards em todos os nodes.

## Imutabilidade

Uma execução referencia uma versão/variante fixa do modelo.

## Garbage collection

Artefatos não utilizados podem ser removidos segundo pressão de armazenamento, frequência e prioridade.

## Performance

Resultados de execução relacionam modelo, variante, backend e hardware para alimentar o modelo de desempenho.

## Compilação

No futuro, o Model Manager pode produzir artefatos derivados otimizados para classes específicas de hardware.
