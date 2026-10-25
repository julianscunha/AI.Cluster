---
title: "Subsistemas"
aliases: ["Mapa de subsistemas", "02-subsistemas"]
tags: [sistema/moc]
type: sistema/moc
status: vivo
implementacao: parcial
---

# Subsistemas

Mapa dos seis grupos de subsistemas do AI Cluster. Cada grupo tem um MOC próprio. Cada nota descreve a arquitetura-alvo completa do subsistema e, no fim, o que o MVP já faz (`O que já existe`) e o que falta (`todo`). Voltar para [[system]].

## Grupos

| Grupo | Responde | Conteúdo |
|---|---|---|
| [[substrato]] | O que é um nó e o que ele faz | Nó e hardware, capability, saúde e confiança, worker runtime, Work Unit, camada de transporte |
| [[execucao]] | Como o trabalho é distribuído | Execution Graph, Execution Planner, Scheduler, modelo de desempenho, hot-plug e recuperação |
| [[dados]] | Onde o estado mora | State Manager, Storage Intelligence, Graph Engine |
| [[ia]] | Como uma requisição de IA é atendida | Inference API, Request Manager, Edge Optimizer, Model Manager, Inference Engine |
| [[plataforma]] | Como o sistema é provisionado e protegido | AI Cluster OS, provisionamento, segurança |
| [[operacao]] | Como o sistema é observado e operado | Observabilidade, CLI e TUI |

## Dependências entre grupos

- [[execucao]] depende de [[substrato]]: sem nós, capabilities e Work Units não há o que escalonar.
- [[dados]] e [[ia]] dependem de [[execucao]]: estado e inferência rodam como execuções.
- [[plataforma]] prepara os nós que o [[substrato]] usa.
- [[operacao]] observa todos os grupos e não entra no caminho de execução.

Leitura recomendada: substrato, depois execução, dados, IA, plataforma e operação. Os protocolos entre controller e workers estão em [[protocolo-controller-worker]], e as decisões em [[decisoes]].

> [!warning] Estado do MVP
> O caminho de inferência roda no gateway, fora do modelo de subsistemas alvo: o modelo é executado no processo do gateway e só as projeções vão a workers. Os desvios e a migração estão em [[ia]] e nas notas de cada subsistema.
