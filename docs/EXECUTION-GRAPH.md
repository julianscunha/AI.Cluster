# Grafo de Execução

## Objetivo

Representar o trabalho como um grafo executável de operações e dependências.

## Separação

```
Model Graph
   ↓
Execution Graph
   ↓
Placement Plan
   ↓
Work Units
```

O Execution Graph responde **o que** precisa ser executado. O Placement Plan responde **onde**.

## Vértices

Cada vértice representa uma operação ou etapa executável.

## Arestas

Arestas representam:

- dependência de dados;
- ordem;
- sincronização;
- produção/consumo de tensor.

## Implementação atual

O primeiro núcleo do Execution Graph está implementado no Controller.

Uma `WorkUnitSpec` representa uma Work Unit mais suas dependências e requisitos de backend. O `ExecutionGraph` valida IDs, dependências inexistentes e ciclos, e calcula níveis topológicos. Work Units no mesmo nível podem ser tratadas como independentes.

O `PlacementPlanner` transforma o grafo lógico em um `PlacementPlan`. Para cada nível, os candidatos elegíveis são filtrados por estado, endpoint e backend requerido; depois o Scheduler global os ranqueia. Work Units independentes são distribuídas entre nodes distintos quando houver nodes disponíveis. Em níveis sequenciais, um mesmo node pode ser reutilizado.

O plano contém, para cada Work Unit, o `node_id`, a posição no ranking e o score utilizado pelo Scheduler. O hardware permanece fora do Execution Graph.

O `SubmitWork` já passa por esse fluxo antes da reserva de capacidade e da chamada ao Worker.

## Paralelismo

Operações independentes podem executar em paralelo:

```
       W1 ──┐
       W2 ──┼──→ SYNC → W4
       W3 ──┘
```

## Subgrafos

O Planner pode particionar o grafo em subgrafos conforme hardware e topologia.

## Replanejamento

Quando um node entra, sai ou se degrada, o Execution Graph lógico pode permanecer; o Placement Plan recebe nova versão.

## Épocas

Execuções autoregressivas podem usar epochs para pontos seguros de replanejamento.

```
Epoch 70 → Plan 281
Epoch 71 → Plan 281
Epoch 72 → Plan 282
```

## Speculative Execution

Uma Work Unit identificada como straggler pode ser executada em paralelo em outro node, desde que a política permita.

## Uso fora de LLM

O mesmo mecanismo deve suportar processamento de documentos, áudio, visão, embeddings, reranking e outros workloads.

## Critério de projeto

O Execution Graph deve ser independente do hardware físico e o Placement Plan deve ser independente do histórico bruto de telemetria.

## Execução de DAG

O Controller já executa um Execution Graph com múltiplas Work Units por níveis topológicos.

O fluxo é:

```
Graph
  ↓
Placement Plan
  ↓
Level 0 → execução concorrente
  ↓
Level 1 → execução concorrente
  ↓
...
```

Cada nível só é iniciado após a conclusão bem-sucedida do nível anterior. Falhas impedem o avanço para os níveis seguintes. As reservas de capacidade são criadas por nível e liberadas após a conclusão das Work Units.

Nesta etapa, depends_on representa uma dependência de controle e ordenação. A propriedade data_from representa uma aresta de dados explícita e também cria a dependência de execução necessária.

Para cada data_from, o Controller aguarda a conclusão da Work Unit de origem, recupera seu output e o anexa ao payload da Work Unit consumidora. Quando existem múltiplas fontes, os outputs são concatenados na ordem declarada.

Essa é uma representação inicial de buffer genérico. O Controller agora mantém os outputs em um Buffer Store LRU limitado e os referencia por buffer_id. A localização no BufferDescriptor continua registrando o node produtor; a cópia residente no Controller é tratada como cache/relay local. Buffers que ainda possuem consumidores recebem pins de lifetime, impedindo eviction antes de todos os consumidores recuperarem os dados. A transferência física para o Worker ainda não é executada nesta etapa, e não existe zero-copy.

A camada de Buffer Residency agora separa disponibilidade contextual de rota de transporte. Para um destino, o buffer pode estar Local, Cached, Remote ou Missing. A política de acesso pode escolher Local, ControllerCache, Primary ou Recompute conforme custos estimados.

O Worker possui Buffer Store LRU limitado, com capacidade configurável por RAC_BUFFER_STORE_BYTES e pins para impedir eviction enquanto existirem consumidores locais agendados. O Controller informa ao Worker o buffer_id de saída e a contagem de consumidores locais; o Worker registra o output localmente antes de devolver a resposta de execução. Antes de executar a Work Unit consumidora, o Controller verifica HasBuffer e seleciona a referência local quando disponível.

As data edges são enviadas como WorkInput em ordem. Cada entrada é local por buffer_id ou contém bytes inline do Controller Cache. O Worker resolve entradas locais durante ExecuteWork, sem transferi-las novamente pela rede. A materialização ainda concatena os bytes no payload da execução; não é zero-copy. A recuperação direta de buffers remotos de outro Worker ainda não está implementada.

O Buffer Store é deliberadamente separado do Execution Graph: o grafo define dependências; o store administra a disponibilidade temporária dos dados; o Residency Planner decide a fonte lógica de acesso.

## RPC

O protocolo expõe SubmitGraph, que recebe um execution_id e uma coleção de Work Units com depends_on, data_from e required_backends. O Controller constrói o grafo, calcula o placement e executa o DAG.

## Próxima evolução

Implementar transferência direta entre Workers, checksum/integridade, chunking e retomada de transferência, métricas do data plane, persistência por tier, cancelamento cooperativo e replanejamento durante a execução.
