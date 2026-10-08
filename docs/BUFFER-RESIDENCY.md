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

O Controller possui um Buffer Store LRU que mantém uma cópia temporária dos outputs. Essa cópia pode ser considerada CACHED para uma decisão de acesso. A localização do descriptor continua apontando para o Worker produtor como residência primária.

Ainda não existe Buffer Store no Worker para materialização local persistente entre Work Units. Portanto, LOCAL está modelado no domínio, mas não é inferido automaticamente pelo Controller nesta etapa.

Também não existe recomputação automática. O planner já representa Recompute como alternativa de decisão, desde que um custo estimado seja fornecido pela camada superior.

## Próximos passos

A evolução natural é ligar a decisão de residency ao Transport Layer e, posteriormente, ao Tensor/Buffer Engine do Worker. Nessa fase, a decisão poderá selecionar entre reutilização local real, leitura do cache, transferência da residência primária ou recomputação, com métricas observadas substituindo estimativas fixas.
