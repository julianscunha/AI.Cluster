---
title: "Grafo de execução"
aliases: ["Execution Graph", "Grafo de execução"]
tags: [sistema/subsistema/execucao]
type: sistema/subsistema
---

# Grafo de execução

Voltar para [[execucao]].

O grafo de execução representa uma requisição como operações com dependências explícitas. Operações independentes podem rodar em paralelo, em nós diferentes. Uma operação que depende de outra espera exatamente pelo dado de que precisa.

## Elementos

- **Nó do grafo**: uma [[work-unit]], com identificador próprio e ligada a uma requisição.
- **Dependência de controle** (`depends_on`): a unidade só é despachada depois que todas as unidades listadas completam.
- **Ligação de dado** (`matrix_a_from`, `matrix_b_from`): uma entrada da unidade é a saída de outra. A saída só existe depois que a origem completa.
- **Saída intermediária**: resultado retido enquanto alguma unidade não concluída pode lê-lo.

## Montagem

O cliente monta o grafo enviando as unidades com seus vínculos. O RPC `EnqueueGraph` enfileira um grafo de quatro unidades em diamante (A alimenta B e C, que alimentam D), com vínculos reais. O comando `aic cluster graph` envia esse grafo e espera a fila esvaziar.

## Invariantes

1. Nenhuma unidade é entregue antes que suas dependências completem e suas entradas vinculadas estejam disponíveis.
2. Um resultado só é aceito do nó para o qual a unidade foi despachada. Resultado atrasado de nó que perdeu a posse é recusado.
3. Falha de uma unidade falha também seus dependentes, transitivamente. Nenhum dependente fica pendurado.
4. O significado lógico do grafo não depende do placement. Trocar o nó que roda uma unidade não muda o resultado.
5. Uma unidade devolvida à fila mantém dependências e vínculos.

Os checksums das unidades de um diamante são diferentes entre si, porque cada uma consome a saída da anterior. Isso confirma que o dado fluiu pelo grafo.

## Retenção de saídas

Uma saída intermediária fica no controller enquanto alguma unidade na fila ou em voo ainda pode lê-la. Quando nenhuma unidade pendente a lê, ela é descartada. O estado é gravado de forma que essa poda vale para o snapshot de [[state-manager]] também.

## Limites conhecidos

- O resultado intermediário cruza a rede duas vezes: do worker de origem para o controller, e do controller para o worker seguinte. Não há caminho direto entre workers.
- Só há dependência ponto a ponto. Não há barreiras por conjunto de unidades.
- A entrega de uma unidade pronta pode levar até um intervalo de poll para ser notada, e o `claim` percorre a fila inteira.
- Um cliente que espera o fim do grafo faz poll até o próprio prazo. Uma unidade sem nó capaz de rodá-la fica pendente, sem erro.
- Não há versão de plano por grafo.

> Planejado: grafo derivado do modelo e do perfil da requisição (precisão, caches, política), barreiras locais explícitas, caminho direto worker a worker, e consulta de prontidão sem varredura linear.
