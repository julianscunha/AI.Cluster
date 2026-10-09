# Estado da Documentação

## Versão

Arquitetura: v0.1

## Consolidado

- separação entre Application Plane e Compute Plane;
- AI Cluster OS em bare metal;
- x86_64 e ARM64;
- hardware heterogêneo;
- capability e health separados;
- hot-plug e replanejamento;
- Work Unit;
- Execution Graph;
- Scheduler;
- Transport Layer;
- State Manager;
- Model Manager;
- Edge Optimizer;
- Graph Engine;
- Storage Intelligence;
- Observability;
- Inference API;
- Request Manager;
- Inference Engine;
- descoberta real de CPU, memória e armazenamento;
- benchmark inicial de capability no ingresso do Worker;
- RTT e throughput gRPC do Worker para o Controller;
- Scheduler implementado como componente separado;
- seleção inicial de Worker por score;
- controle de carga ativa e reserva/liberação de Work Units;
- execução distribuída inicial de multiplicação matricial;
- shard de linhas de A enviado individualmente a cada Worker;
- benchmark de escalabilidade com comparação de 1 a N Workers;
- validação de integridade dos blocos retornados;
- Execution Graph com dependências e detecção de ciclos;
- cálculo de níveis topológicos para paralelismo;
- Placement Plan separado do Execution Graph;
- filtro de placement por backend requerido;
- placement integrado ao caminho de SubmitWork;
- execução de DAG por níveis topológicos;
- execução concorrente de Work Units dentro do mesmo nível;
- propagação de falha impedindo o avanço para o próximo nível;
- RPC SubmitGraph para submissão de Execution Graph;
- data edges explícitos com transporte inicial de outputs entre Work Units;
- Buffer Store no Controller com cache LRU limitado, metadados de buffer, pins de lifetime e reutilização dos outputs por buffer_id;
- modelo inicial de Buffer Residency separado do Transfer Layer;
- classificação contextual em Local, Cached, Remote e Missing;
- decisão de acesso entre Local, Controller Cache, Primary e Recompute por custo estimado;
- integração do Residency Planner ao preparo das data edges do DAG;
- Buffer Store LRU local no Worker, com capacidade configurável e pins de lifetime;
- RPCs PutBuffer, GetBuffer e HasBuffer;
- WorkInput com referência a buffer local ou bytes inline, mantendo a ordem das data edges;
- armazenamento local de outputs no Worker produtor antes da resposta de ExecuteWork;
- fallback pelo Controller Cache quando a cópia local não está disponível;
- transferência Worker-to-Worker acionada pelo Controller, com payload peer-to-peer;
- RPCs TransferBuffer e ReleaseBufferPin para transferência e ciclo de vida de pins;
- escolha entre ControllerRelay e WorkerToWorker no Residency Planner.

## Experimental

- melhor estratégia de paralelismo;
- granularidade ideal de tensor;
- desempenho real de USB;
- contribuição de celulares;
- política final de swap/cache;
- backend de GPU/NPU;
- armazenamento definitivo;
- modelo de performance calibrado com dados históricos;
- pesos finais do Scheduler;
- custo de recomputação calibrado;
- seleção dinâmica entre cache e residência primária baseada em telemetria real.

## Próxima etapa

Adicionar checksum/validação de integridade, chunking, retry/resume, métricas peer-to-peer reais, persistência por tier, cancelamento cooperativo e replanejamento do Placement Plan durante a execução.
