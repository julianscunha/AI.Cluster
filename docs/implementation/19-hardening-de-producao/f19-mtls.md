---
title: "F19 — mTLS"
aliases: ["mTLS", "certificado de cliente"]
tags: [implementacao/fase/19, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-19-hardening-de-producao]]"
sistema: ["[[seguranca]]", "[[adr-016-mtls-opcional-no-plano-de-controle]]"]
evidencia: ["code/tests/phase-t-mtls.sh"]
---
# F19 — mTLS

- [ ] Superado

## Objetivo

Um peer sem certificado de cliente da CA do cluster não passa nem do handshake, mesmo com o token de join.

## O que existe

- Controller: `AI_CLUSTER_TLS_CLIENT_CA` liga a exigência (`client_ca_root`). Sem certificado de servidor, não sobe.
- Cliente compartilhado (`protocol::open_channel`): `AI_CLUSTER_TLS_CLIENT_CERT` e `AI_CLUSTER_TLS_CLIENT_KEY`, em par. Vale para worker, CLI e gateway.
- `phase-t-mtls.sh`: certificado certo entra (worker e CLI); sem certificado cai em `CertificateRequired`; de outra CA, em `UnknownCA`; meio par é recusado pelo cliente; `CLIENT_CA` sem certificado de servidor não sobe.

## Limites

- O certificado prova "pertence ao cluster", não "é o nó X": a ligação com o `node_id` falta.
- Sem rotação, sem revogação (CRL) e sem emissão automática; os certificados são gerados à mão.
- Só o canal controller↔clientes; a perna HTTP do gateway segue sem TLS.
