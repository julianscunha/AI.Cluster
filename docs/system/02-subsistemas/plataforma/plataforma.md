---
title: "Plataforma"
aliases: ["Plataforma", "Sistema e segurança"]
tags: [sistema/moc, sistema/subsistema/plataforma]
type: sistema/moc
---

# Plataforma

Subsistema que cobre o sistema operacional dos workers, a forma como uma máquina vira worker e como o cluster garante identidade, autorização e confiança. Voltar para [[subsistemas]].

## Notas

- [[ai-cluster-os]]: sistema mínimo e imutável de cada worker, com atualização A/B, release assinada e rollout com canário.
- [[provisionamento]]: como uma máquina vira worker, pelo bootstrap sobre Linux existente, pela imagem em cartão ou pelo boot de rede.
- [[seguranca]]: identidade, papéis de credencial, registro e aprovação, transporte, mTLS opcional, atestação de software, release assinada, revogação e auditoria.

## Ordem de leitura

[[ai-cluster-os]], [[provisionamento]] e [[seguranca]]. A imagem define o que o worker é, o provisionamento leva a máquina até ele, e a segurança decide o que o worker pode fazer.

## Resumo para quem opera

- Três tokens de cluster (join, operador e inferência) e uma credencial por worker. Os tokens são variáveis de ambiente.
- Controller e gateway recusam subir sem os segredos obrigatórios, e recusam segredos iguais.
- TLS é o padrão. Sem certificado, o controller só sobe com `AI_CLUSTER_INSECURE=1`.
- Release de imagem assinada com Ed25519. O worker confere antes de gravar.
- A imagem armv7 tem A/B com recuperação por rede. Não há imagem para outras arquiteturas.
