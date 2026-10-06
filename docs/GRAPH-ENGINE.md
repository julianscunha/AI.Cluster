# Engine de Grafos

## Objetivo

Representar e percorrer relações que sejam úteis para busca contextual, topologia, execução, dependências e diagnóstico.

## Grafos previstos

```
Knowledge Graph
Hardware Graph
Network Graph
Execution Graph
State Graph
Observability Graph
```

## Entidades

Nodes podem representar equipamentos, modelos, shards, requests, Work Units, documentos, eventos, métricas e outros objetos relevantes.

## Relações

Exemplos:

```
CONNECTED_TO
DEPENDS_ON
CONTAINS
USES
PRODUCES
CONSUMES
RUNS
STORED_ON
REPLICATED_ON
RELATED_TO
AFFECTS
SIMILAR_TO
```

## Traversal

Consultas devem começar por entidades e seguir relações relevantes, evitando recuperar conteúdo indiscriminadamente.

## Projeção

O grafo operacional é uma projeção dos dados primários. Deve ser reconstruível.

## Hardware

O grafo descreve nós, dispositivos, memória, enlaces e aceleradores.

## Scheduling

Pode fornecer proximidade de recursos, dados e shards para o Planner.

## Edge Intelligence

Pode localizar entidades, dependências, incidentes, ferramentas e contexto relevante.

## Observabilidade

Correlações entre request, Work Unit, node, métrica e evento permitem diagnóstico causal.

## Performance

Índices derivados podem manter consultas frequentes de proximidade e disponibilidade.

## Regra

Grafo não é sinônimo de busca mais rápida. O mecanismo deve usar traversal quando a estrutura relacional realmente reduzir o espaço de busca.
