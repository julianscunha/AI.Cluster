---
title: "F07 — TLS"
aliases: ["Fase S4", "S4", "tls"]
tags: [implementacao/fase/07, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-07-seguranca-base]]"
codigos-antigos: ["S4"]
sistema: ["[[seguranca]]"]
evidencia: ["code/tests/phase-s4-tls.sh"]
---
# F07 — TLS

- [ ] Superado

> [!note] Contexto
> Com a S1 e a S3 os segredos (join, operador e credencial por nó) passam a ser checados, mas
> ainda cruzam a rede em claro. Esta fatia põe TLS nos dois serviços de gRPC, para que os
> segredos não sejam lidos no caminho.

## Objetivo

Cifrar o canal entre controller e clientes (workers, CLI, gateway) e falhar fechado quando não
houver certificado.

## O que foi feito

- O controller serve TLS de um sentido com `AI_CLUSTER_TLS_CERT` e `AI_CLUSTER_TLS_KEY` (PEM,
  sempre em par). Sem eles ele **não sobe**, a menos que `AI_CLUSTER_INSECURE=1` (aviso alto).
- Com certificado e `INSECURE` juntos vale o certificado: um opt-out esquecido não rebaixa.
- O cliente (worker e CLI) usa `https://` e confere a CA de `AI_CLUSTER_TLS_CA` (obrigatória
  nesse caso). `AI_CLUSTER_TLS_DOMAIN` troca o nome verificado quando o endpoint é um IP.
- `http://` só com `AI_CLUSTER_INSECURE=1`: o cliente também falha fechado.
- Vale para os dois serviços (`ClusterControl` e `LinkProbe`). A medição de rede (C2) passa a
  incluir o custo do TLS, então a linha de base de `network_achieved_score` muda.

## Decisões

- **TLS de um sentido.** O servidor se apresenta; o cliente não apresenta certificado. A
  autenticação continua sendo o segredo (credencial por nó). mTLS é a evolução, em
  [[fase-19-hardening-de-producao]].
- **Opt-out explícito e ruidoso.** `INSECURE` existe para laboratório e não rebaixa um controller
  que já tem certificado.

## Evidência

`bash code/tests/phase-s4-tls.sh`:

- https com a CA certa entra.
- http puro e CA errada são recusados por erro de transporte, não de token.
- http sem `INSECURE` o cliente recusa.
- controller sem certificado não sobe.

Os demais scripts de `code/tests` rodam com `AI_CLUSTER_INSECURE=1`.

## Limites e próximos passos

- **TLS de um sentido, sem mTLS.** O cliente não apresenta certificado; a credencial por nó
  continua sendo o segredo.
- **Sem rotação nem revogação de certificado:** troca-se os PEM e reinicia.
- **Porta de métricas 9101** exige `Authorization: Bearer <token de operador>` (S5, 401 sem ele),
  mas sem TLS.
- **Chave em arquivo e em volume.** Sem cofre de segredos ([[fase-19-hardening-de-producao]]).
- **Gateway:** a perna HTTP da API não tem TLS (ver [[f08-gateway-e-api]]).

## Relacionados

- Fase: [[fase-07-seguranca-base]] · Sistema: [[seguranca]]
- Irmãos: [[f07-token-de-join]], [[f07-operador-e-credencial-por-no]], [[f07-aprovacao-de-no]]
