---
title: "Dashboard — painel de uma tela"
type: implementation
status: implemented
tags: [implementation, dashboard, observability, cli]
---

# Dashboard — painel de uma tela

As métricas do H1 respondem "como está o cluster" em `chave=valor`, e o `nodes` responde "como
está cada nó" numa tabela. Operar exige as duas juntas: um nó que caiu importa junto do trabalho
que parou de andar, e uma reserva ativa explica por que um nó ocioso não recebeu trabalho. Isto
é o H4 da fase de Observability: juntar o estado do cluster numa vista só, que se atualiza.

## O que mudou

- `ai-cluster dashboard [--watch N]` na CLI, que lê três chamadas que o controlador já expõe:
  `ListNodes`, `GetMetrics` e `ListReservations`. Não há RPC novo: o painel é apresentação, não
  uma quarta fonte de verdade.
- Sem `--watch` a vista é impressa uma vez e o comando sai. Com `--watch N` ela se repete a cada
  N segundos até Ctrl-C, limpando a tela entre uma volta e outra para não virar um log.

## Como ler

```
PAINEL nós_disponíveis=1/1

NÓ                   ESTADO       THREADS    RAM_GB  COMPUTE    SAÚDE
node-d               READY              4      3.77     1.23     0.76

TRABALHO concluídas=2 pendentes=0 bloqueadas=0 falhas=0
TEMPO compute_total_us=3533 compute_media_us=1766 latencia_total_us=10000 latencia_media_us=5000 previsto_total_us=36

RESERVAS 0
```

- `nós_disponíveis` é o que o placement pode escolher agora; a diferença para o total é nó que
  está drenando, offline ou em quarentena.
- `ESTADO` é a palavra que o controlador usa (`READY`, `DRAINING`, ...). O painel não colore
  linhas: o texto do estado já diz o que importa, e uma cor sozinha não sobrevive a um terminal
  sem cor ou a um `| grep`.
- `concluídas` é cumulativo desde o start do controlador, não uma taxa por segundo. Um painel
  que mostrasse taxa precisaria de uma janela deslizante, que o controlador não guarda.
- `TEMPO` separa compute de latência: a distância entre os dois é a transferência.

## Por que não aparece tudo que a spec pede

O painel da spec (`docs/22-cli-tui.md`) lista TTFT, ITL, tokens/s, tokens evitados, KV cache e
previsto vs real. Os de inferência ainda não têm fonte: TTFT/ITL/tokens são do motor de
inferência (Fase L/M). "Previsto vs real" deixou de ser um deles: o `GetMetrics` (H1) passou a
somar a previsão (`total_predicted_compute_us`), então a linha `TEMPO` traz `previsto_total_us`
ao lado de `compute_total_us` — a leitura de cluster do que o `request trace` (H5) dá por
requisição. Um campo sem fonte fica de fora em vez de aparecer zerado — zero é um valor, e
"não medido" não é zero. Quando as fontes de inferência existirem, cada uma entra como uma
seção a mais, sem mexer nas que já estão aqui.

## Teto conhecido

- **Repinta por volta inteira.** Cada atualização relê as três chamadas do zero; não há streaming
  nem push do controlador. Para um painel de operação é suficiente, e um `--watch 2` já mostra o
  cluster andando.
- **Não há histórico na tela.** É o estado de agora; uma faixa de tempo exigiria série temporal,
  que o H1 deliberadamente não guarda.
- **Terminal limpo, não curses.** A tela é repintada com sequências ANSI simples. Não há
  navegação, paginação nem seleção de nó — quem precisa de detalhe de um nó usa
  `ai-cluster node show <id>`, e quem precisa do caminho de uma requisição usa
  `ai-cluster request trace <id>`.

## Evidência

`bash code/tests/phase-h-dashboard.sh`, com um controlador e um worker:

- A) cluster ocioso: `nós_disponíveis=1/1`, `node-d` listado, `concluídas=0`, `RESERVAS 0`;
- B) depois de duas unidades: `concluídas=2` e `compute_total_us > 0`, com `previsto_total_us`
  na mesma linha `TEMPO`;
- C) com `reserve node-d req-panel`: `RESERVAS 1` com a requisição dona na tela.
