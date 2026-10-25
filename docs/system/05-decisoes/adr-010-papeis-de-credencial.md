---
title: "ADR-010 — Papéis de credencial"
aliases: ["ADR-010", "Papéis de credencial"]
tags: [sistema/adr, subsistema/plataforma]
type: sistema/adr
status: decidido
implementacao: implementado
fases: ["[[fase-07-seguranca-base]]"]
---

# ADR-010 — Papéis de credencial

Data: 2026-10-23 a 2026-10-24 · Voltar para [[decisoes]].

## Contexto

Um segredo único compartilhado por nós, CLI e clientes permitia que quem tinha o token aprovasse a si mesmo, e que um nó comprometido forjasse resultados de outro. A revisão de S2 apontou isso. Também não havia TLS, então os segredos cruzavam a rede em claro.

## Decisão

Cada papel tem um segredo próprio, falha fechada e enxerga só o que precisa:

| Papel | Segredo | Alcança | Não alcança |
|---|---|---|---|
| Join | `AI_CLUSTER_JOIN_TOKEN` | Toda RPC dos dois serviços (`ClusterControl`, `LinkProbe`). Quem não apresenta recebe `Unauthenticated`. | Qualquer RPC administrativa |
| Operador | `AI_CLUSTER_OPERATOR_TOKEN`, diferente do de join e obrigatório no controller | RPCs administrativas: `ApproveNode`, `EnqueueWork`, `EnqueueGraph`, `Reserve`, `ReleaseReservation`, `GetOutput`, `revoke`, `/metrics` | O canal `ShardChannel` do worker |
| Inferência | `AI_CLUSTER_INFERENCE_TOKEN` (opcional, diferente dos outros) | Só `ShardMatmul` | Qualquer outro caminho: `PermissionDenied` |
| Credencial por nó | 128 bits gerados no `RegisterNode` e enviados em `x-ai-cluster-node-auth` | Heartbeat, saúde, `RequestWork`, `SubmitWorkResult`, `LeaveNode` e a sessão `ShardChannel` do próprio `node_id` | Qualquer outro `node_id` |
| Chave de API do gateway | `AI_CLUSTER_API_KEY`, em `Authorization: Bearer` | HTTP do gateway ([[inference-api]]) | Controller e workers, que não recebem essa chave |

Regras de falha fechada:

- O controller não sobe sem join e operador. Um token vazio no servidor não aceita ninguém.
- O gateway não sobe sem chave de API, nem com ela igual a algum dos três tokens do cluster.
- O `InferenceGate` envolve os serviços do controller. Quem apresenta o token de inferência e chama qualquer outro caminho que não seja `ShardMatmul` recebe `PermissionDenied`. Um RPC novo fica fora do alcance desse token até alguém liberá-lo explicitamente.
- A comparação de segredos não tem saída antecipada (`same_token`).
- Aprovação manual é o padrão. Um nó aprovado só troca de credencial com `revoke`.

Transporte: TLS de um sentido (S4), com a CA verificada pelo cliente. Não há mTLS nesta fase.

## Consequências

- Quem tem o token de join não aprova nó nem gera trabalho.
- Um worker que perde a credencial volta a pendente, a menos que `AI_CLUSTER_NODE_CREDENTIAL_FILE` esteja configurado.
- Rotação: reiniciar o controller com novos tokens (join e operador), trocar os PEM e reiniciar (certificado), ou `revoke` (credencial por nó).

## Alternativas

- Segredo único compartilhado: estado de S1. Descartado: quem tinha o token aprovava e forjava resultados.
- Identidade por `device_id` informado pelo cliente: substituída pela credencial por nó (S3b), porque o cliente podia forjá-lo.
- mTLS por nó: adiado para [[fase-19-hardening-de-producao]].

> [!warning] Tetos
> Os segredos ficam em texto claro no controller (arquivo de nós com modo 0600) e nas variáveis de ambiente (visíveis em `docker inspect`). A credencial de nó não tem rotação periódica. Quem tem o token de join ainda consegue registrar um `node_id` alheio pendente. A porta de métricas exige o token de operador, mas não tem TLS. O gateway tem uma única chave, sem tenant nem cota.

> [!info] O que já existe
> Os cinco papéis estão implementados nos commits de S1 a S5 e no papel de inferência, com `phase-s1-join-token.sh`, `phase-s2-approval.sh`, `phase-s4-tls.sh` e `phase-j-inference.sh` como evidência. Detalhes em [[f07-token-de-join]], [[f07-aprovacao-de-no]], [[seguranca]] e [[f08-gateway-e-api]].

> [!todo] mTLS, attestation, rotação e auditoria estão em [[fase-19-hardening-de-producao]]. Tenant, cota e ACL por modelo estão em [[fase-11-request-manager]].
