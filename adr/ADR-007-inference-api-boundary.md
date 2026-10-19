# ADR-007 — API como fronteira entre Application Plane e AI Cluster

Status: DECIDED

## Contexto

O AI Cluster é uma infraestrutura especializada em otimização e execução de cargas de IA. Agentes, aplicações e orquestradores possuem responsabilidades diferentes, como interação com usuário, uso de ferramentas, workflows e lógica de negócio.

Expor o Data Plane diretamente faria o cluster acumular responsabilidades que não são sua especialidade e criaria acoplamento com cada agente.

## Decisão

O AI Cluster será consumido por meio de uma **Inference API**. Nenhum cliente externo acessará diretamente Controller, Worker, Model Shard, Tensor ou State Store.

Agentes como Hermes, Codex, aplicações próprias e orquestradores poderão consumir o cluster através da API.

A interface inicial buscará compatibilidade com APIs de inferência de LLM amplamente utilizadas, sem obrigar o protocolo interno a seguir a mesma representação.

## Consequências

Positivas:

- desacoplamento entre Application Plane e Compute Plane;
- menor consumo de recursos do cluster;
- liberdade para mudar topologia e scheduler;
- integração com múltiplos agentes e aplicações;
- segurança e autorização centralizadas;
- observabilidade por request.

Negativas:

- uma camada adicional de API;
- necessidade de manter compatibilidade do contrato externo;
- possível overhead de serialização no gateway.

## Regra

A Inference API é a única fronteira suportada para workloads externos. Interfaces de administração e protocolos internos permanecem separados.
