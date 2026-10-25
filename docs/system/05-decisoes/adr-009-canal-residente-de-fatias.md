---
title: "ADR-009 — Canal residente de fatias"
aliases: ["ADR-009", "J5", "Canal residente de fatias"]
tags: [sistema/adr, subsistema/ia]
type: sistema/adr
status: decidido
implementacao: parcial
fases: ["[[fase-08-inferencia]]"]
---

# ADR-009 — Canal residente de fatias

Data: 2026-10-24 · Voltar para [[decisoes]].

## Contexto

Um token gerado por um LLM pequeno (SmolLM2-135M) faz cerca de 210 multiplicações de matrizes dependentes, em série. Medido localmente, um token leva cerca de 105 ms em uma thread de CPU. Três caminhos ruins foram medidos ou calculados antes da decisão:

- enviar os pesos a cada chamada: 28 MB por camada em f64, inviável;
- a fila de trabalho atual (de puxar, com polling): 54 a 160 ms por requisição de três níveis, o que dá 4 a 10 s por token, 40 a 100 vezes pior que o local;
- um canal residente com pesos carregados e ativações trafegando: cerca de 2,5 ms por chamada, 120 chamadas por token, ~300 ms por token. Mais lento que o local, mas viável para distribuir o modelo.

O modelo distribuído precisa existir quando o modelo não cabe num nó, e o custo de latência tem de ficar visível.

## Decisão

1. **Fatiar as projeções por faixas da saída.** Cada projeção é cortada em bandas iguais, uma por nó, no eixo das linhas de saída (`[out, in]`). Nas projeções do GPT-2 (`Conv1D`, `[in, out]`) o corte é por colunas de saída. A entrada é replicada, então não há all-reduce.
2. **Pesos residentes no disco do worker.** Cada worker lê do próprio `model.safetensors` só a faixa que lhe cabe, na primeira vez que ela é pedida, e a mantém em `ShardStore`, com despejo LRU (`AI_CLUSTER_SHARD_CACHE_MB`, padrão 2048). A chave da fatia é `(subdiretório, tensor, linhas)`, sem versão. Pesos não trafegam pela rede.
3. **Canal bidirecional aberto pelo worker.** O worker abre o stream `ShardChannel` até o controller, então não precisa de porta de entrada. O controller só repassa: recebe `ShardMatmul` do gateway e envia `ShardCompute` ao nó. O nó precisa estar registrado, aprovado, vivo e apresentar a credencial exata. Keepalive HTTP/2 de 10 s nos dois lados detecta conexão meio aberta. Sessão nova substitui a antiga.
4. **Estado no gateway.** O gateway guarda o KV cache, os embeddings, as normas e a projeção de saída. Projeções com a mesma entrada (q, k e v; gate e up) vão numa chamada só.
5. **Failover no gateway.** Quando um `ShardMatmul` falha por causa de um nó (canal fechado, indisponível, prazo, erro de cálculo), o controller devolve o erro com `x-ai-cluster-failed-node`. O gateway tira esse nó de cena por 30 s, entrega as faixas dele aos vivos e refaz a chamada, que é pura. Sem nenhum nó vivo, a resposta é 503. Cada tentativa tira mais um nó, então o laço termina.
6. **Limites de controle no relay.** Fila de 64 quadros e 64 operações em voo por nó, `ShardMatmul` simultâneos limitados a 8 (`ResourceExhausted` acima), prazo de 8 s por `ShardMatmul` no controller e 30 s por pedido no gateway. Cancelamento (cliente que sai) é propagado.

## Consequências

Positivas:

- modelos maiores que um nó passam a rodar sem reenviar pesos;
- falha de um worker no meio de uma geração não corrompe a continuação, porque o KV cache está no gateway (verificado em `phase-j-shards.sh`);
- qualquer worker serve qualquer faixa, então reatribuir não copia nada.

Negativas:

- latência por token maior que a local (cerca de 3 vezes para SmolLM2 em contêineres numa máquina);
- o RTT real entre nós soma a cada chamada e não foi medido em rede física;
- a primeira chamada de cada fatia é lenta (leitura do disco);
- é uma superfície nova de protocolo, com autenticação, quotas, carga e descarte de fatias;
- o token de inferência escolhe nó, subdiretório e faixa (ver o review em `f08-engine-local`).

## Alternativas

- **Pipeline por camada.** Não medida. Num decode de uma sequência os estágios são sequenciais: cada token atravessa todos, e a latência de rede se soma por estágio sem ganho de paralelismo. Fica como possível extensão para modelos que não cabem num nó ([[inference-engine]], seção de paralelismo de pipeline).
- **Enviar pesos pela rede.** Descartada: 28 MB por camada em f64, a cada chamada.
- **Executar o modelo inteiro no worker.** Não distribui memória, então só serve se o modelo cabe num nó. O gateway continuaria dono do KV cache e da geração.
- **Reutilizar a fila de trabalho.** Medida: 4 a 10 s por token. A fila foi feita para lote independente, não para o caminho crítico de um decode.

> [!warning] Teto e divergência do MVP
> Com o modo distribuído ligado, o gateway ainda roda a geração e o modelo está no próprio processo: o que se distribui são as projeções. O modo é opt-in (`AI_CLUSTER_SMOLLM2_SHARDS`, `AI_CLUSTER_GPT2_SHARDS`); o padrão continua local.

> [!info] O que já existe
> Canal `ShardChannel`, `ShardStore`, leitura por faixa (`read_rows`, `read_band`), relay no controller, failover no gateway e papel de inferência ([[f08-engine-local]], [[fase-08-inferencia]]). Paridade: logits idênticos ao local em 3 nós para SmolLM2 e GPT-2 (`sharded_projections_match_the_local_forward`). E2E em `code/tests/phase-j-shards.sh`.

> [!todo] Sem réplica de fatias pré-carregada, sem rebalanceamento quando um nó entra, sem pipeline entre camadas, sem all-reduce, sem medição de RTT real. O gateway ainda não tem Request Manager ([[request-manager]]). Ver [[fase-12-model-runtime]] e [[canal-de-fatias]].
