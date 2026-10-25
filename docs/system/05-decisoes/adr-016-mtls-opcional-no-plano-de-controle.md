---
title: "ADR-016 — mTLS opcional no plano de controle"
aliases: ["ADR-016", "mTLS"]
tags: [sistema/adr, subsistema/seguranca]
type: sistema/adr
status: decidido
implementacao: parcial
fases: ["[[fase-19-hardening-de-producao]]"]
---

# ADR-016 — mTLS opcional no plano de controle

Data: 2026-10-25 · Voltar para [[decisoes]].

## Contexto

O TLS de um sentido (S4) prova o controller ao cliente, mas qualquer um que alcance a porta e tenha o token de join conversa com ele. Os tokens podem vazar; um certificado de cliente é uma segunda prova, checada antes de qualquer byte de aplicação.

## Decisão

1. **A CA de clientes liga o mTLS.** `AI_CLUSTER_TLS_CLIENT_CA` no controller exige certificado de cliente assinado por ela, em todo o canal (workers, CLI e gateway). Sem a variável, o comportamento de S4 não muda.
2. **O cliente apresenta o par.** `AI_CLUSTER_TLS_CLIENT_CERT` e `AI_CLUSTER_TLS_CLIENT_KEY` (sempre em par) entram no `open_channel` compartilhado, então worker, CLI e gateway ganham o mTLS no mesmo lugar.
3. **Sem certificado de servidor, não sobe.** `AI_CLUSTER_TLS_CLIENT_CA` sem `AI_CLUSTER_TLS_CERT/KEY` é erro, e não um mTLS silenciosamente desligado.
4. **Os tokens continuam.** O certificado é a camada de baixo; join, operador, inferência e a credencial por nó seguem valendo por cima.

## Consequências

- Um peer sem certificado, ou com certificado de outra CA, cai no handshake (`CertificateRequired`, `UnknownCA`) sem chegar a nenhum token.
- Fica fora desta decisão: associar o certificado ao `node_id` (hoje o certificado prova "faz parte do cluster", não "é este nó"), rotação e revogação de certificados (sem CRL), e a emissão automática pela CA do cluster.
