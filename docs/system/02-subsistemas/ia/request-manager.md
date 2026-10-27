---
title: "Request Manager"
aliases: ["Gerenciador de requisições", "Request Manager"]
tags: [sistema/subsistema/ia]
type: sistema/subsistema
---

# Request Manager

O Request Manager controla o ciclo de vida externo de uma requisição de IA: quem pode pedir, quanto pode usar, em que ordem é atendida e como é cancelada. Decide **se e em que condições** uma requisição executa. O [[scheduler]] decide **como** os recursos concretos são usados. Voltar para [[ia]].

Hoje essa função vive no gateway, em `crates/gateway/src/request_manager.rs`. Ver [[adr-015-request-manager-no-gateway]].

## Tenants

- `AI_CLUSTER_API_KEY` cria o tenant `default`, com todos os modelos e sem cota.
- `AI_CLUSTER_TENANTS` acrescenta outros tenants, no formato `nome=chave[,models=a|b][,tokens=N][,class=c];...`.
- `models` é a ACL de modelo. Sem ela, o tenant vê todos.
- `tokens` é a cota por janela. Sem ela, o tenant não tem cota.
- `class` é a classe máxima de prioridade que o tenant pode pedir.
- Nomes de tenant aceitam letras, dígitos, `_` e `-`.

## Cota de tokens

- Um pedido de chat gasta prompt mais tokens gerados. Um embedding gasta um token a cada quatro bytes de entrada.
- A janela é `AI_CLUSTER_QUOTA_WINDOW_SECS` (padrão 3600 s).
- A cota é conferida antes da geração e de novo depois da espera na fila. Assim, uma rajada que esperava com a cota já gasta não passa inteira.
- A conta é feita ao fim da geração. O último pedido da janela pode ultrapassar a cota pelo tamanho dele.
- Cache hit também é cobrado, com os tokens do prompt e da resposta devolvida. Assim o cache não contorna a cota ([[edge-optimizer]]).

## Fila por classe

A geração de chat é atendida uma de cada vez, em ordem de classe:

```text
realtime > interactive > normal > batch > background
```

- Dentro da classe, a ordem é de chegada.
- Quem espera sobe uma classe a cada 5 s, para evitar inanição.
- O cliente pede a classe em `x-priority`. Sem o cabeçalho vale `normal`.
- A fila tem até `AI_CLUSTER_QUEUE_MAX` pedidos (padrão 8) e espera até `AI_CLUSTER_QUEUE_WAIT_SECS` (padrão 10 s). Além disso, a resposta é 429.
- Quem desconecta enquanto espera sai da fila na hora.
- Embeddings não entram nesta fila. Usam um limite de quatro em andamento.
- O grafo de embedding entra na fila do controller com a classe do pedido.

## Cancelamento

- Um embedding que termina sem entregar resposta (prazo, falha ao consultar ou cliente que saiu) pede ao controller o cancelamento. As unidades ainda na fila são removidas, e o controller registra um evento `request_cancelled`.
- Unidade já em execução num worker não é interrompida. Ela termina, e o resultado é descartado.
- O operador pode cancelar por CLI: `ai-cluster request cancel <id>` ([[cli-tui]]).
- Chat não tem cancelamento explícito. Desconectar cancela a geração no próximo token.

## Prazos

Cada embedding e cada chat têm prazo fixo de 30 s. Estourar o prazo responde 504 `TIMEOUT`.

## Métricas

O `GET /metrics` do gateway traz, por tenant, os tokens cobrados e os pedidos recusados (`ai_cluster_tenant_*{tenant="..."}`). Cada tenant vê só os próprios contadores ([[observabilidade]]).

## Limites conhecidos

- **Estado só na memória do gateway.** Cotas, contadores e fila somem no reinício. Não há coordenação entre gateways.
- **Fila só para chat.** Embeddings têm limite de concorrência, sem fila.
- **Prioridade só na espera.** A classe ordena quem espera. Não interrompe nem reordena uma execução em curso.
- **Admissão por contagem.** O gateway não estima tokens de saída nem o custo de memória ou de KV cache antes de admitir.
- **Sem equidade entre tenants** dentro de uma mesma classe.
- **Sem quota por modelo.** A cota é uma só por tenant, somando todos os modelos.

> Planejado: estados persistidos da requisição (`RECEIVED`, `ADMITTED`, `QUEUED`, `RUNNING`, `STREAMING`, `CANCELLED`, `COMPLETED`, `FAILED`, `REJECTED`, `EXPIRED`) e eventos por transição.
> Planejado: orçamento de tokens e de recursos antes da execução, com admissão por carga do cluster.
> Planejado: equidade entre tenants com aging, e quota persistida entre reinícios.
> Planejado: prazo calculado (`remaining_budget`) em vez de prazo fixo.
> Planejado: `Idempotency-Key`, para que um cliente que repete após timeout não crie outra execução.
> Planejado: cancelamento até as unidades em execução.
