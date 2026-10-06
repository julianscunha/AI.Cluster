# Requisitos

## Requisitos funcionais

### RF-01 — Ingresso automático
Um equipamento Linux compatível deve conseguir ser descoberto, autenticado, avaliado e provisionado sem configuração manual repetitiva.

### RF-02 — Reimagem
Após adoção, o sistema poderá apagar o ambiente anterior e instalar o AI Cluster OS em bare metal.

### RF-03 — Multi-arquitetura
O runtime deve suportar x86_64 e ARM64 como arquiteturas de primeira classe.

### RF-04 — Hardware heterogêneo
CPU, GPU e NPU diferentes devem ser tratados por uma camada de abstração comum.

### RF-05 — Medição de capacidade
Cada node deve possuir baseline e capacidade medida por benchmark, além de capacidade corrente derivada de telemetria.

### RF-06 — Hot-plug
Nodes podem entrar, sair ou falhar sem exigir reconfiguração global manual.

### RF-07 — Replanejamento
A entrada, saída ou degradação relevante de um node pode gerar novo plano de execução.

### RF-08 — Recuperação
O cluster deve conseguir restaurar estado suficiente para continuar uma execução conforme a política de recuperação.

### RF-09 — Execução distribuída
Uma única requisição pode utilizar múltiplos nodes.

### RF-10 — API
Consumidores externos usam apenas a Inference API.

### RF-11 — Observabilidade
O sistema deve apresentar estado, saúde, capacidade, desempenho, tokens, latência, comunicação, cache e histórico.

### RF-12 — Otimização de borda
O sistema deve poder evitar LLM, reduzir contexto, reutilizar cache e selecionar somente as ferramentas/capacidades necessárias.

### RF-13 — Grafos
Grafos devem estar disponíveis para traversal contextual, topologia e relações de execução.

### RF-14 — Armazenamento inteligente
O AI Cluster OS deve detectar HDD, SATA SSD, NVMe e adaptar políticas de cache, staging, swap e estado.

## Requisitos não funcionais

### RNF-01 — Determinismo operacional
Componentes de controle devem ter estados e transições explícitos.

### RNF-02 — Versionamento
Protocolos, OS, runtime, modelos e planos devem possuir versões verificáveis.

### RNF-03 — Segurança
Comunicação entre Controller e Worker deve utilizar autenticação mútua e autorização por capacidade/escopo.

### RNF-04 — Baixo overhead
Agentes de controle e telemetria não devem competir significativamente com a carga de IA.

### RNF-05 — Explicabilidade
Decisões do Planner e do Scheduler devem possuir razões e métricas.

### RNF-06 — Recuperabilidade
Falhas de node, processo ou comunicação devem ter comportamento previsível.

### RNF-07 — Elasticidade
Adicionar capacidade deve atualizar a estimativa de capacidade do cluster.

### RNF-08 — Eficiência
O sistema deve considerar não apenas tokens por segundo, mas tokens evitados, comunicação evitada e computação evitada.

## Critérios de aceitação do MVP

O MVP inicial será considerado válido quando:

- dois workers forem registrados automaticamente;
- heartbeat e detecção de perda funcionarem;
- hardware e capacidade forem reportados;
- uma Work Unit puder ser enviada a um worker;
- uma operação matricial puder ser distribuída;
- tempo de computação e comunicação forem medidos separadamente;
- um node puder sair e o estado de controle permanecer consistente;
- a CLI mostrar o estado do cluster em tempo quase real.
