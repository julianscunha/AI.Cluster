---
title: "Substrato"
aliases: ["Substrato", "Camada de hardware"]
tags: [sistema/moc, subsistema/substrato]
type: sistema/moc
---

# Substrato

O substrato transforma máquinas heterogêneas em workers com capacidade medida, saúde observada, confiança explícita e um caminho de dados até o controller. É a base sobre a qual [[execucao]] e [[dados]] funcionam. Voltar para [[subsistemas]].

## Notas

- [[no-e-hardware]]: identidade do worker, descoberta de hardware e desaparecimento de workers.
- [[capability-engine]]: medidas de compute, rede, armazenamento e memória que alimentam o placement.
- [[saude-e-confianca]]: saúde operacional, separada da confiança (autorização e integridade).
- [[worker-runtime]]: componentes do worker, ciclo de trabalho, execução local e variáveis de ambiente.
- [[work-unit]]: a menor unidade atribuível pelo scheduler, com campos, ciclo de vida e tentativas.
- [[camada-de-transporte]]: gRPC sobre HTTP/2, limites de mensagem, sonda de link e canal de fatias.
