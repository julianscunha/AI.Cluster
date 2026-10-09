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
3. em empate, a preferência é Primary (transferência WorkerToWorker), depois Controller Cache e depois Recompute;
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

O Controller mantém um Buffer Store LRU para os outputs e o Worker mantém seu próprio Buffer Store LRU limitado. O limite padrão do Worker é 128 MiB e pode ser configurado por RAC_BUFFER_STORE_BYTES. Os outputs são armazenados no Worker produtor antes que ExecuteWork retorne quando há consumidores agendados, incluindo consumidores remotos, desde que haja capacidade local.

Work Units podem referenciar buffers locais por buffer_id. Ao executar, o Worker resolve cada referência local e consome um pin; inputs sem cópia local são enviados como dados inline no RPC ExecuteWork. Isso preserva a ordem de concatenação de data_from mesmo quando uma Work Unit mistura inputs locais e inline.

O Controller consulta HasBuffer antes de escolher Local. A disponibilidade local é protegida por pins de lifetime; se a cópia local não puder ser instalada, o Controller Cache permanece como caminho de fallback. A resolução local ainda materializa bytes dentro do payload da execução e não é zero-copy.

A transferência Primary agora pode ser executada diretamente entre Workers. O Worker produtor envia o buffer ao endpoint do consumidor e ambos mantêm pins durante o ciclo de vida do input. Se a transferência falhar, o Controller usa o cache como fallback. A recomputação automática ainda não existe.

## Integração atual

O prepare_graph_work consulta o Residency Planner e verifica a disponibilidade local no consumidor e no produtor. Quando a decisão é Primary, o Controller envia apenas o comando de transferência ao Worker produtor; o payload segue diretamente do Worker produtor ao Worker consumidor via RPC PutBuffer. Os bytes não passam pelo Controller nesse segundo caminho.

Se o Worker produtor não possuir a cópia, se a rota direta falhar ou se o buffer local do consumidor não tiver sido materializado, o Controller usa seu Buffer Store como fallback. O TransferPlan distingue ControllerRelay de WorkerToWorker. As estimativas de custo ainda usam medições de rede worker-to-controller como aproximação e precisam ser calibradas com métricas peer-to-peer reais. O planner favorece WorkerToWorker em empate porque evita encaminhar a segunda cópia de dados pelo Controller.

## Medição peer-to-peer e integridade

O Worker Runtime expõe PeerProbe e ProbePeerLink. O Worker de origem mede o RTT de ida e volta e a taxa observada de payload echo diretamente com o destino, usando até três amostras de payload de 256 KiB. O Controller consulta essas medições sob demanda e mantém o resultado em cache por par direcionado durante 30 segundos. Cada medição, falha e timeout também é gravado em `data/peer-link-history.json`; o caminho pode ser alterado por `RAC_PEER_LINK_HISTORY_PATH`. O histórico mantém até 32 observações por par direcionado e descarta, ao carregar, observações com mais de 24 horas. RTT e throughput são agregados a partir das últimas amostras bem-sucedidas; se a sonda atual falhar, o Planner pode usar uma estimativa histórica recente. A taxa recente de falhas/timeout penaliza o custo estimado do caminho direto. Essa taxa é uma medição gRPC round-trip, não uma promessa de throughput unidirecional sustentado.

Cada buffer recebe um checksum SHA-256 no formato `sha256:<hex>`. O Worker de destino valida o checksum antes de armazenar o buffer; o Worker produtor confirma ao Controller o checksum aceito. Inputs inline e locais também são validados antes de serem consumidos. A execução retorna DataLoss quando a integridade não confere.

As métricas peer-to-peer alimentam a estimativa de custo quando disponíveis. Se a sondagem falhar, o Controller registra o evento e pode aproveitar medições bem-sucedidas do histórico persistido nas últimas 24 horas, com penalidade proporcional à taxa recente de falhas; sem histórico válido, usa a estimativa baseada na capability reportada. O arquivo de histórico é um estado operacional local do Controller e deve ser incluído na estratégia de backup/configuração da instalação.

## Próximos passos

Validar a calibração da penalidade de confiabilidade, medir perda/jitter em amostras repetidas e ampliar os testes de falha e replanejamento. A retomada de chunks existe em memória durante a vida do processo; reinicialização do Worker exige reiniciar a transferência do offset zero. Evoluir a recuperação local para eliminar cópias desnecessárias sem declarar zero-copy prematuramente.
