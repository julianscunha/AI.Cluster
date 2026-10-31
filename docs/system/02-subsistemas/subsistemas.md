---
title: "Subsistemas"
aliases: ["Mapa de subsistemas", "02-subsistemas"]
tags: [sistema/moc]
type: sistema/moc
---

# Subsistemas

Mapa dos seis grupos de subsistemas do AI Cluster. Cada grupo tem uma nota própria, e cada nota descreve o que existe hoje e, em linha marcada com `> Planejado:`, o que ainda não existe. Voltar para [[system]].

## Grupos

| Grupo | Responde | Conteúdo |
|---|---|---|
| [[substrato]] | O que é um worker e o que ele faz | Worker e hardware, capability, saúde e confiança, worker runtime, Work Unit, camada de transporte |
| [[execucao]] | Como o trabalho é distribuído | Execution Graph, Execution Planner, Scheduler, modelo de desempenho, hot-plug e recuperação |
| [[dados]] | Onde o estado mora | State Manager, Storage Intelligence, Graph Engine |
| [[ia]] | Como uma requisição de IA é atendida | Inference API, Request Manager, Edge Optimizer, Model Manager, Inference Engine |
| [[plataforma]] | Como o sistema é provisionado e protegido | AI Cluster OS, provisionamento, segurança |
| [[operacao]] | Como o sistema é observado e operado | Observabilidade, CLI |

## Dependências entre grupos

- [[execucao]] depende de [[substrato]]: sem workers, capabilities e Work Units não há o que escalonar.
- [[dados]] e [[ia]] dependem de [[execucao]]: estado e inferência rodam como execuções.
- [[plataforma]] prepara os workers que o [[substrato]] usa.
- [[operacao]] observa todos os grupos e não entra no caminho de execução.

Leitura recomendada: substrato, depois execução, dados, IA, plataforma e operação. Os protocolos entre controller e workers estão em [[protocolo-controller-worker]], e as decisões em [[decisoes]].

> [!note] Onde a inferência roda hoje
> A inferência não segue à risca o modelo de subsistemas. O gateway recebe a requisição, autentica, admite e executa o modelo no próprio processo. Só as projeções lineares são enviadas aos workers, em faixas residentes. Detalhes em [[ia]].
