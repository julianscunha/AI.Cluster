# ADR-032: velocidade de uma única requisição, sem perda de qualidade

Status: decidido (03/11/2026)

## Contexto

A prioridade de produto é acelerar **uma** requisição (vazão com várias requisições não é prioridade), sem perda de qualidade (nada de quantização menor nem modelo menor) e pensando no pior cenário (hardware lento, Wi-Fi), não no ideal. Medição no lab, Qwen2.5-7B Q4_K_M dividido por camadas em 2 notebooks i7-8565U (4 núcleos, 8 threads, 8 GB):

| Situação | Geração |
|---|---|
| PC do lab no Wi-Fi (RTT ~10 ms) | 1,08 tok/s |
| PC do lab e workers no cabo (RTT <1 ms) | ~1,35 tok/s |
| + decodificação especulativa (Qwen2.5-0.5B, aceita 60%, ~3,4 tokens/passo) | 1,1 a 1,2 tok/s (mais lento) |
| + `rpc-server` compilado para AVX2/FMA | **~2,1 tok/s** (prompt de 2,5 para 10 tok/s) |

Contadores de rede mostram ~56 idas e voltas por token: a sincronização custa ~0,1 s de ~0,66 s. O resto é computação, e o `rpc-server` x86 era compilado para CPU genérica (`-mcpu=x86_64`, só SSE2) mesmo em CPUs com AVX2/FMA.

## Decisão

1. **Variantes do `rpc-server` por CPU.** A imagem x86 leva `ggml-rpc-server` (genérico) e `ggml-rpc-server-avx2` (haswell em diante: `avx2 fma f16c bmi1 bmi2 movbe abm`). O worker escolhe ao subir, lendo `/proc/cpuinfo` (`pick_variant` em `worker/src/rpc.rs`); sem a variante ou sem alguma instrução, usa o genérico. Heterogeneidade preservada: PCs antigos continuam rodando. A resposta é a mesma salvo ruído numérico de ponto flutuante (FMA, ordem de soma): com temperatura 0 o texto pode divergir em palavras, não em qualidade.
2. **Decodificação especulativa opt-in** (`AI_CLUSTER_LLAMA_DRAFT_MODEL`, `_DRAFT_MAX`), desligada por padrão, só para modelo da mesma família. Verificado: com o modelo auxiliar o texto sai idêntico ao sem ele. Em workers limitados por CPU conferir vários tokens por passo custa quase proporcionalmente, então ficou mais lenta; reavaliar com workers mais rápidos.
3. **Quantização menor descartada** (perde qualidade).
4. Mais workers dão memória (modelos maiores), não velocidade, no split por camadas; velocidade que cresce com workers exige paralelismo de tensor (próximo passo, ADR própria).

## Consequências

Compilar duas variantes do `rpc-server` x86 (`build.sh`, `build-x86v3`). O armv7 (Pi) segue com uma só; verificar NEON/dot product é pendência. Medir também em Wi-Fi e com nós lentos antes de declarar números do produto.
