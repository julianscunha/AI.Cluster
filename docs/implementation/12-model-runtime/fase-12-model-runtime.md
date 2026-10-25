---
title: "F12 — Model Runtime distribuído"
aliases: ["Fase N — Model Runtime", "docs/14 (model manager)", "M4"]
tags: [implementacao/fase/12, implementacao/estado/planejado]
type: implementacao/fase
status: planejado
codigos-antigos: ["EP N1–N9", "roadmap 6", "M4"]
sistema: ["[[model-manager]]", "[[inference-engine]]", "[[adr-007-fronteira-inference-api]]", "[[adr-009-canal-residente-de-fatias]]", "[[canal-de-fatias]]"]
---
# F12 — Model Runtime distribuído

## Objetivo

Tratar modelos como artefatos versionados e executá-los em vários nós: registry, manifest com versão e checksum dos pesos, variantes de precisão, shards, cache e planejamento pipeline/tensor/híbrido. O placement deve ser decidido pelo controller, não pelo gateway, e a execução deve sair do processo do gateway.

## Depende de

- [[fase-08-inferencia]]: engine CPU e canal residente de fatias, onde N4 e N6 já foram entregues.
- [[fase-09-transporte]], [[fase-10-inteligencia-de-storage]], [[fase-11-request-manager]], [[fase-06-estado-e-recuperacao]].

## Entrega

- Registry de modelos, manifest, variantes e precisão (N1–N3).
- Sharding e cache de modelo (N4, N5).
- Backend CPU (N6, feito em F08).
- Pipeline, tensor e planejador híbrido (N7–N9).
- Placement decidido pelo controller e execução fora do gateway.
- Estado KV distribuído ou persistido (manifesto).

## Checklist

Verificado em `code/crates/engine`, `gateway/src/main.rs` (`shard_nodes`, `AI_CLUSTER_*_SHARDS`), `controller/src/shards.rs` e `worker/src/shards.rs`.

- [ ] N1 Model registry. Modelos são diretórios apontados por variável de ambiente (`AI_CLUSTER_GPT2_DIR`, `AI_CLUSTER_SMOLLM2_DIR`, `AI_CLUSTER_SMOLLM2_INSTRUCT_DIR`). Não há registro.
- [ ] N2 Manifest. Não existe. A configuração vem do `config.json` do modelo, sem versão, lista de tensores nem checksum.
- [ ] Checksum e versão dos pesos. Nenhum hash é verificado. Cada worker lê os pesos do próprio disco.
- [ ] N3 Variantes e precisão. Só `fp32`; o worker reporta `precisions = ["fp32"]` fixo. Não há quantizados.
- [~] N4 Sharding. Projeções de camada são divididas por linhas de saída entre workers (`ShardChannel`, GPT-2 e SmolLM2). Fatias não são pré-carregadas, porque não há réplica.
- [~] N5 Cache de modelo. O worker tem cache de fatias com despejo LRU (`AI_CLUSTER_SHARD_CACHE_MB`). Não há cache de modelo no controller nem prefetch.
- [x] N6 Backend CPU. Engine próprio em CPU com paridade contra `transformers` (GPT-2, SmolLM2 base e instruct). Entregue em [[fase-08-inferencia]] (J2).
- [~] N8 Paralelismo de tensor. Só row-split de projeções, com entrada replicada e sem all-reduce.
- [ ] N7 Paralelismo de pipeline.
- [ ] N9 Planejador híbrido.
- [ ] Placement decidido pelo controller. Hoje o gateway lê `AI_CLUSTER_*_SHARDS` (lista de nós) e manda a carga; o controller só impõe o roster (aprovado, vivo, credencial).
- [ ] Execução fora do gateway. O modelo roda dentro do processo do gateway (~1,6 GB de RAM). Sharding é opt-in.
- [~] Estado KV. Vive no gateway, por geração. Não é distribuído nem persistido. Uma queda de worker é recuperada por reatribuição de faixa e nova chamada, e a geração segue do mesmo ponto.
- [~] Critério de saída: modelo executa em vários nós CPU e o overhead é medido. Executa (SmolLM2, 2 workers, ~2,5 ms por chamada, ~3× mais lento que local), mas não é robusto: sem réplica e sem rebalanceamento.

> [!warning] Desvio do MVP em relação à arquitetura-alvo
> O gateway hoje roda o modelo e escolhe nós por variável de ambiente, sem registro nem checksum dos pesos. A migração prevista é: controller decide placement a partir de registry e manifest; gateway vira só API e roteamento; o runtime do modelo sai do gateway.

## Gate

Gate padrão (EP §19): código, benchmarks, telemetria, testes de falha, atualização de documentação e ADR quando a arquitetura mudar.

Gate específico (manifesto): overhead medido e M4 completo. O critério do EP (modelo robusto em múltiplos nós CPU) exige que a queda de qualquer worker seja tolerada com réplica ou reatribuição, sem reinicialização de sessão.

## Slices

Notas desta pasta (esta nota é o MOC). Entregas já existentes que pertencem a esta fase:

- [[f08-engine-local]]: engine CPU (N6).
- [[f08-canal-residente-de-fatias]]: sharding residente (N4, N8 parcial).

Slices a criar:

- `f12-registry-e-manifest`: N1 e N2, com versão e checksum dos pesos.
- `f12-variantes-de-precisao`: N3, quantizados e precisão declarada por nó.
- `f12-placement-pelo-controller`: placement movido do gateway para o controller.
- `f12-execucao-fora-do-gateway`: runtime do modelo fora do processo do gateway.
- `f12-cache-de-modelo`: N5, cache e prefetch no nível do controller.
- `f12-pipeline-e-planejador-hibrido`: N7 e N9.

## Códigos antigos

- EP N1–N9.
- Roadmap 6 (model registry, model shards, quantized variants, CPU backend, pipeline/tensor/hybrid planning).
- M4 (`[ ]`, Fase 6 do roadmap).
- Pacotes de trabalho da Fase 6 em `phases.md`: manifest, formato de shard, cache, integração de kernel CPU, execução distribuída.
- Absorvido de F08: registry, manifest, versão e checksum dos pesos; placement pelo controller; execução do modelo fora do gateway.

## Specs do sistema

- [[model-manager]]: registry, manifest e versões de modelo.
- [[inference-engine]]: engine de execução e paridade.
- [[adr-007-fronteira-inference-api]]: fronteira entre gateway e controller.
- [[adr-009-canal-residente-de-fatias]] e [[canal-de-fatias]]: canal de fatias.
