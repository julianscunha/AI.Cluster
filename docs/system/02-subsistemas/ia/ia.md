---
title: "IA"
aliases: ["Camada de IA", "Grupo ia", "Inferência"]
tags: [sistema/moc]
type: sistema/moc
---

# IA

Subsistemas do caminho de uma requisição de IA, da aplicação até a execução do modelo. Voltar para [[subsistemas]].

## Caminho da requisição

```text
Aplicação
  -> Inference API          autenticação por chave, ACL de modelo, streaming
  -> Request Manager        tenants, cota de tokens, fila por classe
  -> Edge Optimizer         cache exato de respostas
  -> Model Manager          manifesto e versão dos pesos
  -> Inference Engine       forward do modelo; projeções nos workers
```

> Planejado: Execution Planner e Scheduler no caminho de inferência, com o Model Manager resolvendo variante e shards. Hoje quem escolhe os workers das projeções são as variáveis `AI_CLUSTER_*_SHARDS` do gateway.

## Notas

- [[inference-api]]: fronteira única para aplicações; API compatível com o padrão de chat e de embeddings. Hoje é o gateway HTTP.
- [[request-manager]]: decide se e quando uma requisição executa. Hoje a admissão e a fila por classe estão no gateway; estados persistidos e orçamento de recursos são planejados.
- [[edge-optimizer]]: reduz trabalho antes da inferência. Hoje há só o cache exato de respostas.
- [[model-manager]]: identifica os pesos de cada modelo por diretório, com manifesto de versão e checksums.
- [[inference-engine]]: executa o grafo do modelo em CPU (GPT-2 e SmolLM2, base e instruct), com KV cache e projeções distribuídas.

Protocolo e decisões: [[adr-007-fronteira-inference-api]], [[adr-009-canal-residente-de-fatias]], [[canal-de-fatias]], [[adr-015-request-manager-no-gateway]].

## Limites conhecidos do grupo

- **O modelo roda no processo do gateway.** A memória do gateway cresce com os modelos carregados, e reiniciá-lo interrompe as gerações em curso.
- **Uma geração de chat por vez**, somando todos os modelos. Os embeddings têm até quatro pedidos em andamento.
- **Respostas determinísticas.** A decodificação é gulosa: mesmo prompt, mesma saída. Não há temperatura nem amostragem.
- **Só dois modelos de linguagem**, GPT-2 small e SmolLM2-135M (base e instruct), e um modelo sintético de embeddings.
- **Prazo fixo de 30 s** para cada embedding e cada chat. Estourar o prazo gera 504 e cancela a geração no próximo token; o slot só libera quando a geração termina.
