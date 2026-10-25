---
title: "F11 — Tenants, cotas e fila por classe"
aliases: ["Request Manager no gateway", "tenants", "cota de tokens", "fila por classe"]
tags: [implementacao/fase/11, implementacao/fatia, implementacao/estado/parcial]
type: implementacao/fatia
status: parcial
superado: false
fase: "[[fase-11-request-manager]]"
codigos-antigos: ["tetos de f08-gateway-e-api"]
sistema: ["[[request-manager]]", "[[inference-api]]", "[[seguranca]]"]
evidencia: ["code/tests/phase-m-request-manager.sh"]
---
# F11 — Tenants, cotas e fila por classe

- [ ] Superado

## Objetivo

O gateway tinha uma chave só, nenhuma cota e um semáforo que respondia 429 a quem chegasse com a geração ocupada. Esta fatia põe o primeiro Request Manager dentro do gateway (`crates/gateway/src/request_manager.rs`): quem pede (tenants), o que pode usar (ACL de modelo e cota de tokens) e em que ordem é atendido (fila por classe).

## O que foi feito

- **Tenants.** `AI_CLUSTER_API_KEY` continua valendo como o tenant `default` (todos os modelos, sem cota). `AI_CLUSTER_TENANTS="nome=chave[,models=a|b][,tokens=N][,class=c];..."` acrescenta outros. A chave de um tenant não pode repetir a de outro nem os tokens do cluster (join, operador, inferência). A autenticação compara todas as chaves em tempo constante.
- **ACL de modelo.** Um tenant só vê em `GET /v1/models` o que pode chamar; chamar o resto dá 403 `MODEL_FORBIDDEN`, no chat e nos embeddings.
- **Cota de tokens.** `tokens=N` é o limite de tokens (prompt mais gerados) por janela (`AI_CLUSTER_QUOTA_WINDOW_SECS`, 3600). Gasta a cota, o pedido seguinte recebe 429 `QUOTA_EXCEEDED` com `Retry-After` igual ao que falta da janela. A conta é feita ao fim da geração, então o último pedido da janela pode passar da cota pelo tamanho dele; a cota é conferida de novo depois da espera na fila, para uma rajada que esperava com a cota já gasta não passar toda. Embeddings também gastam a cota (um token a cada 4 bytes de entrada).
- **Fila por classe.** A geração (um pedido por vez) é entregue por classe: `realtime` > `interactive` > `normal` > `batch` > `background`, o mais antigo primeiro dentro da classe, e quem espera sobe uma classe a cada 5 s. O cliente pede a classe em `x-priority`; ela nunca passa do teto do tenant (`class=`), e sem pedido vale `normal`. Espera máxima `AI_CLUSTER_QUEUE_WAIT_SECS` (10) e no máximo `AI_CLUSTER_QUEUE_MAX` (8) esperando; além disso, 429 `CAPACITY_UNAVAILABLE`.
- **Prioridade até o controller.** O grafo de embeddings entra na fila do controller com a classe do pedido como `priority` (F04).
- **Cancelamento do grafo.** O RPC `CancelRequest` (só operador; `ai-cluster request cancel <id>`) tira da fila as unidades da requisição. O gateway o chama quando um pedido de embeddings termina sem entregar (prazo, falha ao consultar, cliente que saiu), por um guard que roda no `Drop`, e o controller registra `request_cancelled`.
- **Métricas por tenant** no `/metrics` do gateway: tokens cobrados e pedidos recusados (`ai_cluster_tenant_*{tenant="..."}`). Cada tenant vê só os seus contadores; nomes de tenant são limitados a letras, dígitos, `_` e `-`.

## Evidência

`bash code/tests/phase-m-request-manager.sh` (precisa de `code/models/gpt2`): A) ACL e 401; B) quarto pedido de um tenant de 30 tokens dá 429 com `Retry-After`, outro tenant segue e os contadores aparecem; C) com a geração ocupada, o `interactive` que chegou depois termina antes do `batch` que chegou antes; D) embeddings sem worker dão 504 e o controller registra `request_cancelled req-0 removidas=4`. Unitários: parsing de tenants, teto de classe, janela da cota, autenticação por chave e ordem da fila.

## Limites

> [!warning] O que ainda não é um Request Manager completo
> Não há estados de requisição persistidos, idempotência (`Idempotency-Key`), equidade entre tenants, orçamento de recursos nem quota por modelo. A cota é por processo do gateway e some no restart. A fila só ordena a geração de chat (um por vez); os embeddings ainda usam o semáforo de 4. Uma unidade já no worker não é cancelada: ela termina. Quem desconecta enquanto espera sai da fila na hora (guard no `Drop`) e, se o slot já tinha sido entregue a ele, o repassa.

## Próximos passos

- Cancelamento até o worker (unidade em execução) e do `ShardMatmul` em voo.
- Estados e idempotência da requisição, e cota persistida.
- Equidade entre tenants dentro da mesma classe.
