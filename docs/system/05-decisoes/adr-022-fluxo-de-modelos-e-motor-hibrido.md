---
title: "ADR-022 — Fluxo de modelos guiado pelo controller e motor híbrido"
aliases: ["ADR-022", "catálogo de modelos", "motor híbrido"]
tags: [sistema/adr, subsistema/ia]
type: sistema/adr
---

# ADR-022 — Fluxo de modelos guiado pelo controller e motor híbrido

Status: decidido · Data: 2026-10-28 · Voltar para [[decisoes]].

## Contexto

Hoje um modelo exige variáveis por modelo (`*_DIR`, `*_SHARDS`, `*_SUBDIR`), pesos copiados à mão para cada worker e um gateway configurado à mão. O produto esperado é outro: o usuário põe o modelo numa pasta do controller, escolhe qual iniciar, o controller confere se o cluster comporta (OK ou NOK com o motivo), envia os pesos aos workers, mostra as chaves de API e o benchmark, e repete o processo a cada worker novo. O motor próprio cobre duas famílias (GPT-2, Llama), guarda pesos em f64 e serializa uma geração por vez.

## Decisão

1. **Control plane nosso, data plane híbrido.** O controller fica com catálogo, OK/NOK, placement, chaves, benchmark e segurança. A execução do modelo passa a usar o llama.cpp (GGUF, quantização, centenas de arquiteturas) nos workers; o motor próprio vira referência de paridade. O `rpc-server` do llama.cpp não tem autenticação e só roda atrás do canal autenticado do cluster, nunca em porta aberta.
2. **Formatos:** GGUF é o caminho principal; safetensors do Hugging Face também é aceito, com conversão opcional.
3. **Rede entre workers** (pipeline): o worker abre porta de entrada com mTLS amarrado ao `node_id`; atrás de NAT, o controller repassa. Isto revisa a regra de só saída do [[adr-009-canal-residente-de-fatias]].
4. **Prioridade de desempenho: vazão** (pipeline e vários pedidos em paralelo, ganho quase linear). Tensor-split só entra se medido em cabo e fica desligado em Wi-Fi.
5. **OK/NOK por RAM:** `precisa = pesos no tipo de execução + KV(contexto, lote) + margem`; `tem` = soma da RAM livre dos nós aprovados. O NOK diz quantos GB faltam.

## Etapas

E0 pesos menores na memória · E1 catálogo (`models list`) · E2 OK/NOK (`models check`) · E3 receitas encaixáveis com paridade por receita · E4 pesos por gRPC, sem `scp` · E5 placement automático, sem `*_SHARDS` · E6 `models start` com chaves e benchmark · E7 nó novo recebe o modelo e refaz o benchmark · E8 pipeline entre workers · E9 vários pedidos e micro-batching · E10 tensor-split opcional · E11 quantização própria, se ainda couber. O progresso fica em [[pendencias]].

## Consequências

- **Alvo: hardware comum e fraco** (SBCs ARM, celulares, PCs antigos, pouca RAM e pouco disco, rede Wi-Fi ou 100 Mbps). O desenho e os testes partem do Pi de 1 GB, não de servidores; a quantização (GGUF) deixa de ser otimização e vira requisito. **Não há teto de tamanho por desenho:** qualquer modelo roda se a soma dos recursos do cluster comportar; o NOK só ocorre por falta de recursos e diz quanto falta (GB de RAM, nós a acrescentar). O projeto escala juntando nós.

- Mais workers escalam a **vazão** quase linearmente; a latência de um pedido só melhora até 3 a 5 vezes e depende da rede. Os números são estimativas de banda e de RTT, a medir no cluster.
- Passa a existir dependência de um motor externo (C++ via subprocesso ou FFI) e uma porta de entrada nos workers.
- Em armv7 com 1 GB, o Pi serve a modelos pequenos e a vazão, não a modelos grandes.

## Alternativas

Só o motor próprio: controle total, mas cada receita e a quantização ficam por nossa conta. candle: cobre cerca de 30 modelos escritos à mão, sem kernel NEON para armv7 e sem distribuição.
