---
title: "Visão geral"
aliases: ["Overview", "Visão geral do sistema"]
tags: [sistema/visao]
type: sistema/visao
---

# Visão geral

Voltar para [[visao]]. Princípios em [[principios]]; requisitos em [[requisitos]]; arquitetura em [[arquitetura]].

## Propósito

O AI Cluster é um runtime distribuído e adaptativo para IA: uma plataforma Linux-first que transforma hardware heterogêneo reaproveitado em um pool distribuído de computação para IA.

## Ideia central

Uma coleção heterogênea de equipamentos Linux se comporta como um pool lógico de recursos, capaz de executar uma única requisição distribuindo um modelo e suas operações entre diferentes dispositivos.

## O que o sistema deve fazer

- Executar uma única inferência distribuindo um modelo entre múltiplos dispositivos.
- Aceitar hardware heterogêneo: x86_64, ARM64, CPU, GPU, NPU, PCs, servidores, notebooks, SBCs e dispositivos móveis compatíveis.
- Provisionar nós automaticamente e convertê-los para um AI Cluster OS próprio.
- Suportar hot-plug, drain, falha e replanejamento sem derrubar o cluster.
- Medir capacidade real, saúde, temperatura, armazenamento, rede e desempenho.
- Otimizar tokens e computação na borda antes de acionar modelos grandes.
- Usar grafos para topologia, conhecimento, dependências, execução e observabilidade.
- Tratar comunicação, memória e armazenamento como recursos de primeira classe.
- Fornecer TUI/CLI operacional em shell.
- Expor capacidade de inferência somente por uma Inference API.
- Manter Application Plane e Compute Plane desacoplados.
- Aprender com desempenho observado e recalibrar previsões do cluster.

> Planejado: otimização de borda além do cache exato, grafo de conhecimento, TUI full-screen, aceleradores, worker móvel e aprendizado do planejador.

## Cargas de trabalho primárias

- LLM inference;
- embeddings;
- reranking;
- context processing;
- multimodal preprocessing;
- compression;
- deterministic compute;
- custom AI graphs.

## Propriedades operacionais

- onboarding com pouco ou nenhum toque manual (zero/low-touch);
- reimaging;
- hot-plug;
- scheduling consciente de saúde e de desempenho;
- retrieval assistido por grafo;
- otimização adaptativa de tokens;
- TUI detalhada.

## Fora do escopo

- suporte a hardware móvel arbitrário;
- suporte universal a NPU;
- migração transparente de memória de processo arbitrário;
- garantia de escala linear;
- substituir kernels de LLM maduros por um compilador de kernel novo.

## Limites conhecidos

- O cluster se comunica só por gRPC sobre TCP; o socket Unix e a memória compartilhada existem apenas como camada de transporte isolada ([[adr-014-abstracao-de-transporte]]).
- Os nós testados são x86_64 e ARM de 32 bits (armv7). ARM64 não foi testado.
- O gateway executa o modelo no próprio processo; só as projeções podem ser distribuídas ([[adr-007-fronteira-inference-api]]).
- A capacidade reportada é de compute, memória, rede (velocidade do link), saúde e classe de armazenamento. Memória sob carga, latência e aceleradores não são medidos.
