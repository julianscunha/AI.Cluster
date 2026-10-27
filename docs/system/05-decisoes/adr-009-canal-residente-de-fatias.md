---
title: "ADR-009 — Canal residente de fatias"
aliases: ["ADR-009", "Canal residente de fatias"]
tags: [sistema/adr, subsistema/ia]
type: sistema/adr
---

# ADR-009 — Canal residente de fatias

Status: decidido · Data: 2026-10-24 · Voltar para [[decisoes]].

## Contexto

Um token gerado por um LLM pequeno (SmolLM2-135M) faz cerca de 210 multiplicações de matrizes dependentes, em série. Cada uma depende da anterior, então qualquer latência de rede se soma a todas elas. Três caminhos foram avaliados antes da decisão:

- enviar os pesos a cada chamada: 28 MB por camada em f64, inviável;
- a fila de trabalho existente, de puxar com polling: a latência por requisição, multiplicada por 210 chamadas em série, dá segundos por token, dezenas de vezes pior que a execução local;
- um canal residente, com pesos carregados e só ativações trafegando: mais lento que o local, mas viável para distribuir o modelo.

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
- falha de um worker no meio de uma geração não corrompe a continuação, porque o KV cache está no gateway;
- qualquer worker serve qualquer faixa, então reatribuir não copia nada;
- os logits são idênticos aos do caminho local, garantido por teste.

Negativas:

- latência por token maior que a local;
- o RTT entre nós soma a cada chamada, e a latência de rede física ainda não foi medida;
- a primeira chamada de cada fatia é lenta (leitura do disco);
- é uma superfície nova de protocolo, com autenticação, quotas, carga e descarte de fatias;
- o token de inferência escolhe nó, subdiretório e faixa.

## Alternativas

- **Pipeline por camada.** Num decode de uma sequência os estágios são sequenciais: cada token atravessa todos, e a latência de rede se soma por estágio sem ganho de paralelismo. Fica como extensão possível para modelos que não cabem num nó ([[inference-engine]]).
- **Enviar pesos pela rede.** Descartada: 28 MB por camada em f64, a cada chamada.
- **Executar o modelo inteiro no worker.** Não distribui memória, então só serve se o modelo cabe num nó. O gateway continuaria dono do KV cache e da geração.
- **Reutilizar a fila de trabalho.** Descartada: a fila foi feita para lote independente, não para o caminho crítico de um decode.

## Limites

- O modo distribuído é opt-in (`AI_CLUSTER_SMOLLM2_SHARDS`, `AI_CLUSTER_GPT2_SHARDS`); o padrão é local.
- Com o modo ligado, o gateway ainda roda a geração e guarda o KV cache. O que se distribui são as projeções.
- Não há réplica de fatias pré-carregada, nem rebalanceamento quando um nó entra, nem pipeline entre camadas, nem all-reduce.
- Planejado: pipeline para modelos que não cabem num nó ([[canal-de-fatias]], [[inference-engine]]).
