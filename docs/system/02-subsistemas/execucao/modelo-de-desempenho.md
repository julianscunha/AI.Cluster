---
title: "Modelo de desempenho"
aliases: ["Performance Model", "Modelo de desempenho"]
tags: [sistema/subsistema/execucao]
type: sistema/subsistema
---

# Modelo de desempenho

Voltar para [[execucao]].

O modelo prevê quanto tempo de compute uma unidade levará em cada worker. A previsão alimenta o custo do [[scheduler]] e fica registrada junto do tempo real, para comparação.

## Previsão de compute

Para uma unidade de multiplicação de matrizes, a previsão é:

1. **Taxa do worker.** Threads multiplicadas pela taxa por thread. A taxa vem da medida corrente de compute, se existir; senão, do baseline de registro. Sem nenhuma medida, a previsão não existe e é zero.
2. **Taxa típica.** Quando o desvio da medida de registro é de até 0,25, a taxa é corrigida pela razão entre a mediana e o melhor caso da medida. Acima disso, a correção é ignorada.
3. **Tempo.** Operações da unidade divididas pela taxa, mais um custo fixo de preparo por unidade de 600 µs (`AI_CLUSTER_UNIT_SETUP_US`).
4. **Correção aprendida.** Cada worker tem uma média móvel da razão entre o tempo real e o previsto. O peso é 0,3 para cada nova observação, com limites entre 0,05 e 20. Ela é aplicada sobre a previsão. Não é gravada em disco: após reinício, é reaprendida nas primeiras unidades.

Zero em qualquer campo de medida significa "não medido", não "instantâneo".

## Entradas e saídas

Hoje o modelo usa:

- benchmark de compute do worker (ver [[capability-engine]]);
- medida corrente de compute do worker;
- threads do worker;
- tamanho da unidade;
- medidas de rede (no custo de transferência, não na previsão de compute).

Saídas:

- previsão de tempo de compute por unidade, registrada junto do tempo real;
- custo relativo por worker, usado pelo placement.

As previsões e os tempos reais aparecem nos traces de requisição (`GetTrace`) e nas métricas de [[observabilidade]].

## Capacidade do cluster

Não há agregado de capacidade do cluster. O sistema não soma workers para estimar capacidade total. Quando for preciso, latência, vazão, memória, armazenamento e rede devem ser mantidos como dimensões separadas, sem somar RAM e CPU como se fossem um único dispositivo.

## Limites conhecidos

- Só o tempo de compute é previsto. Transferência e espera na fila não são previstas.
- A previsão assume compute linear com as threads. Turbo e SMT deixam a previsão otimista em workers com muitas threads. A pontuação de placement é relativa: a ordem entre workers importa, o tempo absoluto não.
- O custo fixo de preparo é constante, não uma função do tamanho da unidade. Três tamanhos medidos deram valores de preparo muito diferentes entre si, espalhados demais por ruído para sustentar mais de um número.
- Em host compartilhado, a mediana e o melhor caso divergem muito, e a correção pela razão fica desligada. Nesse caso, a previsão depende só do preparo fixo.
- Unidades de workers sem medida de compute não entram na soma de previsão. O total previsto de uma execução fica subestimado quando parte do cluster não tem medida.
- A correção aprendida não é persistida.
- A previsão é um número só: não há intervalo de confiança nem variância.

> Planejado: previsão de transferência e de latência total, capacidade por dimensão, confiança e variância como saída, entradas de modelo, variante, precisão e tamanho de contexto, e bandas de preparo por faixa de tamanho.
