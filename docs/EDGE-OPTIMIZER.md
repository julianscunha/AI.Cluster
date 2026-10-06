# Otimizador de Borda

## Objetivo

Reduzir tokens, contexto, chamadas a modelos grandes e computação desnecessária antes da inferência distribuída.

## Pipeline

```
Request
→ classificação
→ cache
→ grafo
→ seleção de contexto
→ deduplicação
→ compressão
→ resolução de skill/tool
→ compilação do prompt
→ orçamento
→ seleção de modelo
```

## Fast Path

Operações determinísticas podem ser resolvidas sem LLM.

Exemplos: consultas de estado do cluster, cálculos simples e cache exato.

## Cache

- cache exato;
- cache semântico;
- cache derivado do grafo.

## Grafos

O Engine localiza entidades e relações relevantes antes de montar o contexto.

## Seleção de contexto

Somente fatos relacionados à tarefa devem ser promovidos ao contexto do LLM.

## Deduplicação

Informações equivalentes provenientes de múltiplas fontes devem ser consolidadas, preservando origem e confiança.

## Compressão

A redução de contexto deve considerar relevância e qualidade mínima. Não deve remover fatos necessários apenas para reduzir tokens.

## Skills e ferramentas

Somente capacidades relevantes devem chegar ao contexto do LLM.

## Compilador de prompt

Produz uma representação mínima contendo contrato da tarefa, contexto relevante, ferramentas selecionadas e solicitação.

## Orçamento

O sistema estima input, output, contexto máximo, latência e custo computacional antes de executar.

## Decisão adaptativa

Uma otimização só deve ser aplicada quando a economia esperada superar seu próprio custo.

## Métricas

```
tokens_recebidos
tokens_otimizados
tokens_evitados
percentual_de_reducao
inferencias_evitadas
computacao_evitada
cache_hit
graph_hit
```

## Princípio

O objetivo é maximizar trabalho útil por segundo, não apenas tokens por segundo.
