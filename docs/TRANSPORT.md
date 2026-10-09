# Camada de Transporte

## Objetivo

Transportar Work Units, tensores, estado e artefatos entre recursos do cluster sem amarrar a execução a um único meio físico.

## Planos

```
Control Plane
→ comandos, heartbeat, estado e telemetria

Data Plane
→ tensores, ativações, KV cache e dados de execução
```

## Meios suportados

A abstração deve permitir:

- memória compartilhada;
- Ethernet/TCP;
- RDMA quando disponível;
- USB para dispositivos dedicados;
- novos transportes no futuro.

## Grafo de rede

Cada enlace registra largura de banda, latência, jitter, perda, carga e saúde. O Worker Runtime mede RTT de ida e volta e taxa de payload echo diretamente entre os Workers. O Controller mantém a última medição por par direcionado por 30 segundos e usa os valores observados para estimar a transferência. A taxa round-trip não equivale a throughput unidirecional sustentado; medições específicas do data plane devem substituir esta aproximação conforme o protocolo evoluir.

## Decisão de transporte

Antes de transmitir um objeto, o sistema deve verificar:

- destino;
- tamanho;
- localização de cópias;
- largura de banda;
- latência;
- compressibilidade;
- prioridade;
- deadline;
- custo de recomputação.

## Recomputação versus transferência

Se recomputar for mais barato que transportar, a operação pode ser executada localmente.

## Chunking

Objetos grandes devem permitir transferência em blocos, retomada e retry parcial.

## Compressão

Compressão e descompressão só são usadas quando a economia de transferência superar o custo computacional.

## Quantização para transporte

Quando permitido pelo contrato numérico da operação, tensores podem ser convertidos para representação menor durante o transporte.

## Sobreposição

O runtime deve permitir sobrepor transferência, prefetch e computação.

## Integridade

Dados importantes devem possuir checksum e validação no destino. Os buffers usam SHA-256 em formato `sha256:<hex>`; o destino valida o checksum ao receber um buffer, e o Worker valida inputs locais e inline antes de usá-los.

## Prioridades

Tráfego crítico de estado e execução tem prioridade sobre prefetch, cache e tarefas de fundo.

## Topologia

O custo de um caminho deve considerar todos os enlaces do percurso, não apenas o enlace direto.

## USB

USB é tratado como transporte de primeira classe. O desempenho real, estabilidade e latência devem ser medidos durante a operação.
