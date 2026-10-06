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
- RTT gRPC do Worker para o Controller;
- seleção inicial de Worker por score;
- controle de carga ativa e reserva/liberação de Work Units;
- execução distribuída inicial de multiplicação matricial.

## Experimental

- melhor estratégia de paralelismo;
- granularidade ideal de tensor;
- desempenho real de USB;
- contribuição de celulares;
- política final de swap/cache;
- backend de GPU/NPU;
- armazenamento definitivo;
- modelo de performance calibrado com dados históricos;
- pesos finais do Scheduler.

## Próxima etapa

Executar Fase A com múltiplos Linux reais e medir baseline local versus execução distribuída, registrando tempo de comunicação, tempo de compute, speedup e eficiência paralela.
