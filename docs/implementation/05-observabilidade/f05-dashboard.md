---
title: "F05 — Dashboard: painel de uma tela"
aliases: ["dashboard", "H4"]
tags: [implementacao/fase/05, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-05-observabilidade]]"
codigos-antigos: ["H4"]
sistema: ["[[cli-tui]]", "[[observabilidade]]"]
evidencia: ["code/tests/phase-h-dashboard.sh"]
---
# F05 — Dashboard: painel de uma tela

- [ ] Superado

## Objetivo

As métricas (H1) respondem "como está o cluster" em `chave=valor`, e `nodes` responde "como está cada nó" numa tabela. Operar exige as duas juntas: um nó caído importa junto do trabalho que parou, e uma reserva ativa explica por que um nó ocioso não recebeu trabalho. Juntar o estado do cluster numa vista só, que se atualiza.

## O que foi feito

- `ai-cluster dashboard [--watch N]` lê três chamadas que o controlador já expõe: `ListNodes`, `GetMetrics` e `ListReservations`. Nenhum RPC novo: o painel é apresentação, não uma quarta fonte de verdade.
- Sem `--watch`, imprime uma vez e sai, para um script poder ler. Com `--watch N`, repinta a cada N segundos até Ctrl-C, limpando a tela entre voltas.

## Como ler

```text
PAINEL nós_disponíveis=1/1

NÓ                   ESTADO       THREADS    RAM_GB  COMPUTE    SAÚDE
node-d               READY              4      3.77     1.23     0.76

TRABALHO concluídas=2 pendentes=0 bloqueadas=0 falhas=0
TEMPO compute_total_us=3533 compute_media_us=1766 latencia_total_us=10000 latencia_media_us=5000 previsto_total_us=36

RESERVAS 0
```

- `nós_disponíveis` é o que o placement pode escolher agora; a diferença para o total é nó drenando, offline ou em quarentena.
- `ESTADO` é a palavra que o controlador usa (`READY`, `DRAINING`, ...). O painel não colore linhas: o texto já diz o que importa, e cor sozinha não sobrevive a terminal sem cor ou a `| grep`.
- `concluídas` é cumulativo desde o start do controlador, não taxa por segundo. Taxa exigiria janela deslizante, que o controlador não guarda.
- `TEMPO` separa compute de latência: a distância entre os dois é a transferência.

## Decisões

- **Campo sem fonte fica de fora, não zerado.** Zero é um valor, e "não medido" não é zero. Quando as fontes de inferência existirem, cada uma entra como seção nova, sem mexer nas existentes.
- **Previsto vs real na linha `TEMPO`.** `GetMetrics` soma a previsão (H1), então `previsto_total_us` aparece ao lado de `compute_total_us`: a leitura de cluster do que `request trace` (H5) dá por requisição.

## Evidência

`bash code/tests/phase-h-dashboard.sh`, com um controlador e um worker:

- A) cluster ocioso: `nós_disponíveis=1/1`, `node-d` listado, `concluídas=0`, `RESERVAS 0`;
- B) depois de duas unidades: `concluídas=2` e `compute_total_us > 0`, com `previsto_total_us` na mesma linha `TEMPO`;
- C) com `reserve node-d req-panel`: `RESERVAS 1` com a requisição dona na tela.

## Limites

> [!warning] Repinta por volta inteira
> Cada atualização relê as três chamadas; não há streaming nem push. Suficiente para operação (`--watch 2` já mostra o cluster andando).

> [!warning] Sem histórico na tela
> É o estado de agora. Uma faixa de tempo exigiria série temporal, que o H1 deliberadamente não guarda.

> [!warning] Terminal limpo, não curses
> Repintura com sequências ANSI simples. Sem navegação, paginação nem seleção de nó: detalhe de nó vem de `ai-cluster node show <id>`, e caminho de requisição de `ai-cluster request trace <id>`.

> [!todo] Campos de inferência
> TTFT, ITL, tokens/s e KV cache já têm fonte (`/metrics` do gateway, [[f05-metricas-de-inferencia]]), mas o painel só fala com o controller e ainda não os mostra. Tokens evitados não existem.

## Próximos passos

- Seções de inferência quando as fontes existirem.
