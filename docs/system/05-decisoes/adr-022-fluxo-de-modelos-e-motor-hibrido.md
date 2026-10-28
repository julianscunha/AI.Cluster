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

### Decisão pendente do dono

O RPC do llama.cpp serve a nós de 64 bits. Para os nós armv7 há três caminhos, e a escolha define o resto do roadmap:

1. **Dois planos de dados:** x86 e ARM64 usam o RPC do llama.cpp; o armv7 continua no motor próprio (fatias por linhas) ou roda modelos pequenos sozinho com o llama.cpp.
2. **Corrigir o RPC no armv7:** investigar e enviar a correção ao llama.cpp (ou manter um patch). Custo e prazo incertos.
3. **Camada de distribuição própria sobre a biblioteca do llama.cpp:** usar o llama.cpp só para calcular e distribuir pelo nosso canal. É o caminho mais longo.

Recomendação: **1 agora** (entrega a demo em nós de 64 bits e dá ao Pi o papel de nó pequeno com o llama.cpp local) e **2 em paralelo**, medindo o custo de corrigir.
