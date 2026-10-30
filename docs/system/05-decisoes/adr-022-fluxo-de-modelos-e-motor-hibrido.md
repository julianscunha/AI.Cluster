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

**E-1 (primeiro, decide o resto) — feita em 2026-10-28, resultado abaixo: teste de viabilidade do llama.cpp** no Pi armv7 (1 GB) e no x86: compilar, rodar um GGUF Q4 pequeno, medir RAM e tokens por segundo, e testar o `rpc-server` entre o PC e o Pi. Se falhar no armv7, o motor próprio fica para esse alvo. Até o resultado, **E0 e E3 ficam em espera** (melhoram o motor próprio, que pode virar só referência).

E0 pesos menores na memória · E1 catálogo (`models list`) · E2 OK/NOK (`models check`) · E3 receitas encaixáveis com paridade por receita · E4 pesos por gRPC, sem `scp` · E5 placement automático, sem `*_SHARDS` · E6 `models start` com chaves e benchmark · E7 nó novo recebe o modelo e refaz o benchmark · E8 pipeline entre workers · E9 vários pedidos e micro-batching · E10 tensor-split opcional · E11 quantização própria, se ainda couber. O progresso fica em [[pendencias]].

**Marco de lançamento (demo de aceite da E7):** um modelo maior que a RAM de qualquer máquina do laboratório (14B ou 32B quantizado) rodando em 3 ou 4 máquinas velhas pelo fluxo `models start`, com os tokens por segundo medidos e publicados, inclusive os ruins.

## Consequências

- **Alvo: hardware comum e fraco** (SBCs ARM, celulares, PCs antigos, pouca RAM e pouco disco, rede Wi-Fi ou 100 Mbps). O desenho e os testes partem do Pi de 1 GB, não de servidores; a quantização (GGUF) deixa de ser otimização e vira requisito. **Não há teto de tamanho por desenho:** qualquer modelo roda se a soma dos recursos do cluster comportar; o NOK só ocorre por falta de recursos e diz quanto falta (GB de RAM, nós a acrescentar). O projeto escala juntando nós.

- Mais workers escalam a **vazão** quase linearmente; a latência de um pedido só melhora até 3 a 5 vezes e depende da rede. Os números são estimativas de banda e de RTT, a medir no cluster.
- Passa a existir dependência de um motor externo (C++ via subprocesso ou FFI) e uma porta de entrada nos workers.
- Em armv7 com 1 GB, o Pi serve a modelos pequenos e a vazão, não a modelos grandes.

## Alternativas

Só o motor próprio: controle total, mas cada receita e a quantização ficam por nossa conta. candle: cobre cerca de 30 modelos escritos à mão, sem kernel NEON para armv7 e sem distribuição.

## Resultado da E-1 (2026-10-28)

Medido com o llama.cpp (commit `8ca4060`), GGUF Q4_K_M, compilado para x86 genérico (sem `-march=native`) e para armv7 (Cortex-A7, NEON, estático), ambos com `GGML_RPC`:

| Máquina | Modelo | Geração | Leitura do prompt |
|---|---|---|---|
| PC x86, 4 threads (contêiner) | SmolLM2 360M | 37 tokens/s | 189 tokens/s |
| PC x86, 4 threads (contêiner) | Qwen2.5 0,5B | 27,6 tokens/s | 182 tokens/s |
| Orange Pi PC armv7, 1 GB, 4 threads | SmolLM2 360M | 2,6 tokens/s | 4,2 tokens/s |
| PC x86 → outro nó x86 por RPC | Qwen2.5 0,5B | 28 tokens/s | 102 tokens/s |

- **O llama.cpp roda em armv7 com 1 GB**, sem alteração: o Pi gera 2,6 tokens/s com o SmolLM2 360M. É cerca de dez vezes o que o motor próprio entrega hoje no cluster com o mesmo modelo.
- **O RPC entre nós x86 funciona** e custa pouco (28 contra 27,6 tokens/s do modelo local).
- **O RPC com o armv7 não funcionou:** PC x86 → Pi falha logo na conexão (`Remote RPC server crashed or returned malformed response`, o servidor só registra o fechamento) e Pi → Pi no próprio Pi não terminou o teste em mais de 15 minutos (processo parado). A causa não foi isolada: a estrutura das mensagens não usa `size_t` e não achei diferença de largura de palavra. O servidor RPC do llama.cpp é experimental e declara `Never expose the RPC server to an open network`.
- **Os pesos não passam pelo nosso canal:** no RPC do llama.cpp quem carrega o arquivo (o nó principal) envia os tensores aos servidores RPC na carga. Portanto a etapa E4 (envio de pesos por gRPC) só vale se o motor próprio continuar no caminho.

### Plano escolhido: llama.cpp em todos os nós (validado em 2026-10-28)

O dono escolheu corrigir o RPC no armv7 (plano 2). Foram três defeitos do llama.cpp em 32 bits, corrigidos em `code/third_party/llama.cpp/armv7.patch` (commit fixado `8ca4060`):

1. o tamanho da mensagem do cliente ia como `size_t` (4 bytes), e o protocolo manda 8;
2. o mesmo no sentido servidor → cliente (`send_msg`);
3. `GGML_MEM_ALIGN` valia 4 em 32 bits, mas `ggml_tensor` tem `int64_t`; o compilador emite cargas NEON com dica de alinhamento de 8 e o Cortex-A7 dá SIGBUS.

**Resultado:** um cliente x86 usa o Pi (armv7, 1 GB) como servidor RPC e roda o SmolLM2 360M com todas as camadas nele a 2,3 tokens/s (2,6 sem RPC). Com metade das camadas no Pi, 3,8 tokens/s; sozinho no PC, 42. O Pi limita a velocidade de quem o usa: dividir um modelo que já cabe em uma máquina com um nó lento piora a velocidade; o ganho vem quando o modelo não cabe em uma máquina só.

**Limite descoberto:** um cliente de 32 bits não usa um servidor de 64 bits (os ponteiros remotos de 64 bits são truncados no cliente). O nó principal que carrega o modelo e coordena o RPC é de 64 bits; os nós armv7 entram como servidores.

### Decisões que ainda cabem ao dono, para as etapas E5 a E8

1. **Segurança do `rpc-server`:** ele não tem autenticação nem criptografia. Proposta: o worker o escuta só em 127.0.0.1 e o nosso worker o expõe por um proxy com mTLS amarrado ao `node_id` (ADR-022, rede entre workers).
2. **Quem é o nó principal** (carrega o GGUF e roda o `llama-server`): o gateway atual (contêiner no servidor), ou o worker de 64 bits mais forte escolhido pelo controller.
3. **Como o worker executa o `rpc-server`:** processo filho do worker (o controller manda iniciar e parar), com o binário na imagem do SO.

### Fluxo validado no laboratório real (2026-10-28)

PC (x86, contêiner, 4 threads) e Orange Pi PC (armv7, 1 GB, imagem 11.7), mTLS entre o gateway e os dois `rpc-server`, Qwen2.5 1.5B Q4_K_M (986 MB, mais do que a RAM livre do Pi):

| Configuração | Resultado |
|---|---|
| `model start` (OK por RAM: 1,06 GB contra 2,5 GB) até PRONTO | alguns minutos (carga do GGUF e dos tensores ao Pi) |
| 1 vaga, 1 pedido de 32 tokens | 16,7 s (1,9 tokens/s) |
| 2 vagas (`AI_CLUSTER_LLAMA_SLOTS=2`), 2 pedidos juntos de 32 tokens | 24,8 s cada: 2,6 tokens/s no total (+34%), cada resposta 49% mais longa |
| Benchmark do `model status` | 2,5 a 2,9 tokens/s |

Mais workers aumentam a RAM somada; mais vagas aumentam a vazão total à custa da latência de cada pedido. Um worker lento (o Pi) limita todos os pedidos: o ganho do cluster aparece quando o modelo não cabe em uma máquina só.

### Estado das etapas (E0 a E11)

- **Feitas:** E1 (catálogo), E2 (OK/NOK), E5 (plano de workers, sem `--tensor-split` calculado), E6 (`models start/stop/status`, chaves em hash, benchmark), E7 (worker novo entra no plano e o modelo para se faltar RAM), E9 parcial (várias vagas), E11 (quantização pelo GGUF).
- **Descartadas pela decisão do llama.cpp:** E0, E3, E4 (o head empurra os tensores pelo próprio RPC).
- **Pendentes:** E8 (pipeline entre workers com porta de entrada própria só vale se o RPC do llama.cpp não bastar), E10 (tensor-split), certificado por nó, plano de camadas por RAM, imagens x86 com os binários.

### Quem carrega o modelo: o menor conjunto de workers rápidos (medido em 2026-10-28)

Cada worker que entra soma um salto de rede por token. SmolLM2 360M Q4 (32 camadas) com o gateway no PC e o Pi como segundo worker, velocidade do benchmark:

| Divisão de camadas (Pi : PC) | Tokens/s |
|---|---|
| automática (pela memória que cada `rpc-server` informa) | 5,2 |
| 50 : 50 | 4,3 |
| 20 : 80 | 7,4 |
| 6 : 94 | 7,6 |
| 0 : 100 (só o PC) | 38,1 |

Mesmo 6% das camadas no Pi (2 de 32) derrubam a velocidade em cinco vezes: o custo é a latência do salto, não a vazão. Por isso o controller não reparte entre todos: `models::select` ordena os workers por velocidade e usa os mais rápidos **só até a RAM somada comportar o modelo**; os lentos entram apenas quando faltaria memória. Entre os escolhidos, a divisão é proporcional à velocidade, limitada pelo que cada um comporta. Com isso o 360M passou a 28 tokens/s no laboratório (plano só com o PC), 5,4 vezes mais que a divisão automática. A velocidade vem da pontuação de CPU medida no registro, uma aproximação (veja [[pendencias]]).

Medido de novo em 2026-10-29 com `llama-bench` (Qwen2.5 0,5B Q4, PC i5-7500 por Wi-Fi, Pi no cabo, RTT 2 ms): o PC local gera 24,7 tokens/s; o mesmo PC como servidor RPC, 11,2; o Pi sozinho por RPC, 1,4. Com 10% das camadas no Pi cai a 2,1, com 20% a 1,9 e com 50% a 1,7. O prefill (`pp64`) cai de 132 para 73, 21 e 8 tokens/s. O salto para o nó lento domina qualquer divisão, o que confirma a regra do `models::select` e mostra que um pipeline próprio não melhora a latência de um pedido; só poderia aumentar a vazão com vários pedidos, o que o `llama-bench` não mede. Medido com o `llama-server` e 4 pedidos de 32 tokens ao mesmo tempo (mesmo modelo): local, `-np 1` dá 23,5 tokens/s no total e `-np 4`, 45,1; só o PC por RPC, 9,5 e 12,5; PC com 10% das camadas no Pi, 2,8 e 3,3. Vários pedidos em paralelo (`-np`) já multiplicam a vazão (de 1,4 a 1,9 vez), mas com o Pi no caminho a vazão total fica 4 vezes abaixo da do PC por RPC, porque o estágio lento limita todo pipeline. **Decisão: não construir pipeline próprio entre workers** com nós de velocidades tão diferentes; o ganho está em usar `-np` e em só incluir o nó lento quando falta memória.
