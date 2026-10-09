# Buffer Residency

## Objetivo

Separar a pergunta **onde o dado existe** da pergunta **como o consumidor deve obtê-lo**.

O BufferDescriptor.location continua representando a residência primária/canônica do buffer. A camada de residency trabalha de forma contextual, considerando o destino e as cópias disponíveis.

## Estados de residência no destino

```
LOCAL   → existe cópia física no destino
CACHED  → existe cópia utilizável no Controller
REMOTE  → a residência primária existe, mas exige acesso remoto
MISSING → nenhuma fonte utilizável está disponível
```

A residência é contextual. O mesmo buffer pode ser REMOTE para um Worker e LOCAL para outro.

## Fontes de acesso

O planner pode selecionar:

```
Local
ControllerCache
Primary
Recompute
```

A escolha é feita por custo estimado:

1. reutilização local tem custo zero;
2. entre cache, residência primária e recomputação, vence o menor custo disponível;
3. em empate, a preferência é Controller Cache, depois Primary e depois Recompute;
4. sem caminho disponível, o resultado é Unavailable.

Essa política inicial é deliberadamente simples. O custo futuro deverá incorporar largura de banda, latência, compressão, prioridade, deadline, carga de CPU/GPU/NPU, confiabilidade e custo de ocupar memória.

## Separação de responsabilidades

```
BufferDescriptor.location
        ↓
residência primária

Buffer Residency Planner
        ↓
fonte de acesso

Transfer Planner
        ↓
rota e estratégia de transporte

Worker Runtime
        ↓
materialização/consumo físico do buffer
```

O Buffer Residency Planner não executa transferência, não cria cópia física no Worker e não decide o formato de transporte. Isso evita misturar placement de dados com detalhes do data plane.

## Estado atual

O Controller mantém um Buffer Store LRU para os outputs e o Worker mantém seu próprio Buffer Store LRU limitado. O limite padrão do Worker é 128 MiB e pode ser configurado por RAC_BUFFER_STORE_BYTES. Os outputs são armazenados no Worker produtor antes que ExecuteWork retorne, somente quando há consumidores locais agendados.

Work Units podem referenciar buffers locais por buffer_id. Ao executar, o Worker resolve cada referência local e consome um pin; inputs sem cópia local são enviados como dados inline no RPC ExecuteWork. Isso preserva a ordem de concatenação de data_from mesmo quando uma Work Unit mistura inputs locais e inline.

O Controller consulta HasBuffer antes de escolher Local. A disponibilidade local é protegida por pins de lifetime; se a cópia local não puder ser instalada, o Controller Cache permanece como caminho de fallback. A resolução local ainda materializa bytes dentro do payload da execução e não é zero-copy.

Também não existe transferência direta Primary entre Workers nem recomputação automática. O planner ainda representa esses caminhos para evolução futura, mas só Local e ControllerCache são executáveis nesta etapa.

## Integração atual

O prepare_graph_work já consulta o Buffer Residency Planner antes de anexar um output ao payload da Work Unit. Na implementação atual, o único caminho executável é ControllerCache, porque o Controller é o único componente que possui uma cópia de bytes utilizável durante o DAG.

Quando o cache está disponível, ele é selecionado com custo estimado zero no planner de residency. Quando o cache não está disponível, o Controller não inventa uma transferência da residência primária: como o Worker ainda não mantém uma cópia endereçável do buffer, esse caminho permanece não implementado. Quando nenhuma fonte conhecida está disponível, a execução falha com FailedPrecondition.

O TransferPlan continua separado da decisão de residency. A integração física com o data plane ainda não foi feita.

## Próximos passos

Integrar transferência direta entre Workers, checksums, verificação de integridade, chunking/retry para buffers grandes, telemetria da residência e métricas reais de transferência. Evoluir a recuperação local para eliminar cópias desnecessárias sem declarar zero-copy prematuramente.
