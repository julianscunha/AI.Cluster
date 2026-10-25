---
title: "Inference Engine"
aliases: ["Motor de inferência", "docs/25", "25 — Inference Engine", "AI Cluster — Inference Engine"]
tags: [sistema/subsistema/ia]
type: sistema/subsistema
status: decidido
implementacao: parcial
fases: ["[[fase-08-inferencia]]", "[[fase-12-model-runtime]]"]
---

# Inference Engine

O Inference Engine executa um Model Execution Graph sobre os recursos escolhidos pelo [[execution-planner]] e pelo [[scheduler]]. Faz a matemática do modelo: prefill, decode autoregressivo, KV cache, paralelismo e checkpoints. Não decide a topologia global do cluster. Voltar para [[ia]].

## Objetivo

Suportar, progressivamente: prefill; decode autoregressivo com KV cache; paralelismo de tensor, de pipeline e híbrido; quantização; batching e micro-batching; sobreposição de comunicação e computação; decodificação especulativa; checkpoint e recuperação.

## Separação de responsabilidades

| Componente | Responsabilidade |
|---|---|
| [[request-manager]] | Ciclo de vida da request |
| [[execution-planner]] | Estrutura e placement esperado |
| [[scheduler]] | Alocação de recursos |
| Inference Engine | Execução matemática do modelo |
| [[state-manager]] | Persistência e recuperação do estado |

## Fases de inferência

```text
Request -> Tokenização -> Prefill -> criação do KV cache
        -> loop de decode (token N -> modelo -> token N+1) -> saída
```

- **Prefill** processa o contexto de entrada e cria o estado para a geração. Tem mais paralelismo sobre o contexto.
- **Decode** gera um token por vez reutilizando o KV cache. Sensível a latência, localidade do KV, sincronização e largura de banda de memória.

O planner trata prefill e decode como cargas distintas; a topologia ótima de um pode não ser a do outro.

## Grafo do Transformer

```text
Embedding -> [Norm -> Attention -> Residual -> Norm -> FFN -> Residual] x N
          -> Final Norm -> LM Head
```

Nós internos podem ser agrupados em subgrafos otimizados.

## Kernels, tensores e memória

- **Abstração de kernel:** cada operação passa pela camada de abstração de hardware (backends CPU, GPU, NPU). O grafo do modelo não codifica instruções de hardware.
- **Tensor:** `id`, `shape`, `strides`, `dtype`, `precision`, `layout`, `device`, residência de memória, `version` e `checksum` quando necessário. Layouts são tratados pelo runtime para evitar conversões redundantes.
- **Plano de memória** antes da execução: pesos persistentes, buffers de ativação, KV cache, workspace e buffers de comunicação. Buffers temporários são reutilizados por análise de tempo de vida.

## KV cache

Estado de execução associado à sequência, por camada. Administrado em **blocos paginados**, sem exigir um segmento contíguo grande, o que facilita crescimento, movimentação e recuperação.

- Residência HOT/WARM; compressão ou quantização só quando validada.
- Réplica ou checkpoint quando o State Manager exigir; eviction de estado não crítico.

## Decode e paralelismo

- **Decode:** cada passo é `KV lookup → attention → FFN → logits → sampling → próximo token`. O passo seguinte só começa quando o estado necessário está disponível.
- **Tensor parallel:** uma operação é particionada entre workers (`MatMul` em shards A, B, C seguido de `reduce`). Só vale quando o ganho de compute supera comunicação e sincronização.
- **Pipeline parallel:** blocos do modelo atribuídos a grupos de nós em estágios; ativações atravessam os estágios.
- **Híbrido:** combina os dois (ex.: estágio 1 em tensor parallel de A/B, estágio 2 em C). Estratégia preferencial para hardware heterogêneo; validar experimentalmente.
- **Overlap:** computar o chunk atual enquanto transfere o próximo e faz prefetch do seguinte, com filas independentes quando o backend suporta.

## Otimização de requisição única

O projeto prioriza acelerar uma única request: tensor parallel, pipeline, paralelismo de operações, prefetch, overlap e decodificação especulativa. Não depende só de batching.

- **Batching contínuo:** agrupar sequências em execução para aumentar utilização. O primeiro MVP opera com uma request por vez, mas a abstração deve permitir batching.
- **Especulativa:** um modelo draft pequeno propõe tokens e o modelo principal verifica. Decidir pela taxa de aceitação, custo de comunicação, latência do draft e da verificação, e energia/custo. Pode usar hardware fraco para o draft e forte para verificação.

## Amostragem, determinismo e quantização

- **Amostragem** separada do cálculo do modelo: `temperature`, `top_k`, `top_p`, `seed`, política de repetição. O estado de sampling faz parte do Execution State.
- **Determinismo:** seed fixa, kernels controlados e scheduling estável quando suportado; facilita testes, checksum e diagnóstico.
- **Quantização:** o engine conhece as variantes suportadas (pesos Q4, compute INT8/INT4 onde houver, acumulação em precisão maior). Toda conversão é explícita no Execution Graph e contabilizada no custo.
- **Precisão mista:** por operação, desde que o Model Profile e o backend garantam validade.

## Checkpoint e falha

- Checkpoints após estágios seguros, sobretudo em operações longas: `epoch → compute → checkpoint → próximo epoch`. O [[state-manager]] controla persistência e recuperação; o engine fornece os hooks.
- Classes de falha, que o engine não pode esconder: falha recuperável de trabalho, falha de nó, falha de transporte, corrupção de estado, incompatibilidade de modelo e falha numérica.
- **Validação numérica** (shape, NaN/Inf, faixas, checksum, invariantes de kernel) para operações críticas; não ligar indiscriminadamente no caminho quente.

## Instrumentação e hardware

Cada etapa produz `prefill_time`, `decode_time`, `kernel_time`, `memory_time`, `communication_time` e `sampling_time`, com relação a Work Unit e Node. O engine consulta as capabilities locais (tipos suportados, largura de vetor, acelerador, memória e banda) e escolhe kernels compatíveis.

Otimização local (fusão de kernels, reuso de buffers, prefetch, cache) é livre desde que não altere a semântica do plano global. Placement de shards, topologia, replicação e fronteiras de pipeline são do Controller/Planner.

## Admissão e conclusão

Antes de iniciar, o engine recebe um plano com: variante do modelo, plano de execução, shards necessários, precisão, orçamento de memória, requisitos de transporte e referências de estado.

Uma execução só conclui após: token e estado finais fechados, saída confirmada, política de retenção ou liberação de estado aplicada, métricas descarregadas e trace completo.

## MVP alvo e critério de sucesso

MVP alvo: CPU x86_64, uma request por vez, um modelo, Q4/Q8, pipeline parallel, transporte simples de tensor e KV cache básico. Depois: ARM64, GPU, NPU, tensor parallel, planejamento híbrido e especulativa.

Critério: o modelo é maior que a memória de um nó, roda em vários nós, uma única request atravessa os nós, o resultado está correto e a performance é medida. O benchmark compara com um baseline mononó para provar que a distribuição acelera.

> [!info] O que já existe (MVP, fatias F08)
> - Crate `code/crates/engine`: forward do GPT-2 small (float32, safetensors) e da SmolLM2-135M (arquitetura Llama: RoPE, GQA, RMSNorm, SwiGLU; pesos BF16), em CPU, sobre `Matrix` em f64. Trait `LanguageModel` com `forward` com `KvCache` e `generate` guloso com cancelamento. Tokenizador BPE de byte do GPT-2.
> - Paridade com `transformers`: `gpt2_parity.rs`, `smollm2_parity.rs` e `smollm2_chat_parity.rs` exigem o mesmo token mais provável e logits dentro de 1e-3. Os pesos não entram no repositório.
> - KV cache (`KvCache`, um vetor por camada) verificado contra recalcular o prompt inteiro (`cached_generation_matches_recomputing_the_whole_prompt`).
> - Projeções em workers pelo **canal residente de fatias** ([[canal-de-fatias]], [[adr-009-canal-residente-de-fatias]]): cada worker lê do próprio `model.safetensors` só as linhas ou colunas da sua faixa (`read_band`, em `engine/src/safetensors.rs`) e as mantém em `ShardStore`, com despejo LRU (`AI_CLUSTER_SHARD_CACHE_MB`, padrão 2048). SmolLM2 divide por linhas de saída; GPT-2 por colunas. Entrada replicada. Sem all-reduce e sem pipeline entre camadas.
> - Relay pelo controller: `op_id` próprio, fila de 64 quadros, 64 operações em voo por nó, prazo de 8 s por operação no controller (`SHARD_TIMEOUT`).
> - Falha de nó: o controller devolve `x-ai-cluster-failed-node`; o gateway deixa o nó de fora por 30 s (`Roster`), redistribui as faixas e refaz a chamada. A chamada é pura e o KV cache está no gateway, então a geração continua do mesmo ponto.
> - Papel de inferência: `AI_CLUSTER_INFERENCE_TOKEN` autentica só `ShardMatmul`, vale como portão separado do token de operador e não lista nós.
> - Medidas (contêiner de 4 núcleos, prompt de 8 tokens): SmolLM2 local ~156 ms por token, ~105 ms depois de paralelizar a projeção de saída; GPT-2 de 140 para ~109 ms. Fila de trabalho do controller: 54–160 ms por requisição, inviável para decode. Canal residente com 2 workers: ~2,5 ms por chamada e ~300 ms por token, cerca de 3× o local, porque os dois workers dividem os mesmos núcleos. Com RTT real o custo sobe.
>
> Slices: [[f08-engine-local]], [[f08-canal-residente-de-fatias]], [[f08-chat-completions]].

> [!warning] Desvios do alvo e migração
> 1. **Engine não é serviço separado.** Roda em processo dentro do gateway; o gateway depende do crate `engine`. O alvo é um Model Runtime distribuído com o motor fora da API. Migração: [[fase-12-model-runtime]].
> 2. **Sem planejamento no caminho.** Quem escolhe as faixas são variáveis de ambiente (`AI_CLUSTER_*_SHARDS`) e o Roster do gateway, não o [[execution-planner]] e o [[scheduler]]. O engine não recebe um plano com variante, shards, precisão e orçamento de memória. Migração: [[fase-12-model-runtime]].
> 3. **Pesos sem versão nem checksum.** O caminho é fixo (`root/<subdir>/model.safetensors`) e a chave de fatia é `(subdir, tensor, linhas)`. Não há registry, manifest, variante nem verificação de integridade. Migração: [[model-manager]] (fase F12).
> 4. **Sem quantização.** `Matrix` é f64 (cerca de 1 GB para o GPT-2 small); Q4/Q8 não existe. Migração: [[fase-12-model-runtime]].
> 5. **Decode de uma request, greedy.** Sem amostragem, sem batching, sem KV paginado, sem especulativa. O KV cache é um bloco contíguo por geração.
> 6. **Paralelismo só por divisão de linhas de saída.** Sem pipeline e sem híbrido; tensor parallel parcial, sem reduce.
> 7. **Sem checkpoint de execução.** Se o gateway cai, a geração em curso é perdida. A tolerância existe só para falha de worker, por reatribuição.
> 8. **Sem validação numérica.** Não há checagem de NaN/Inf nem de checksum de saída no engine.
> 9. **Tokens especiais e templates** são só do modelo instruct; o engine não tem contrato de template por modelo.

> [!todo] Falta
> - Model Runtime distribuído (registry, shards, variantes, cache, pipeline/tensor/híbrido, estado KV): [[fase-12-model-runtime]].
> - Quantização Q4/Q8 e compute INT8/INT4.
> - Amostragem e determinismo com seed; `temperature`, `top_p` e `stop`.
> - Batching contínuo e KV paginado.
> - Pipeline e híbrido, decodificação especulativa.
> - Checkpoint por epoch e recuperação de execução de inferência ([[state-manager]]).
> - Validação numérica no caminho de verificação.
> - Pool de threads persistente para o matmul de uma linha (hoje cada chamada cria threads).
> - Backends ARM64, GPU e NPU ([[fase-16-aceleradores]]).
> - Baseline mononó contra distribuído, no critério de sucesso.
