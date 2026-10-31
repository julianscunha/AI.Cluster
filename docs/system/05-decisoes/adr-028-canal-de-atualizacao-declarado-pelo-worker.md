---
title: "ADR-028 — Canal de atualização declarado pelo worker"
aliases: ["ADR-028", "canal de atualização", "update_kind", "release só-Linux em cluster misto"]
tags: [sistema/adr, subsistema/plataforma, subsistema/protocolo]
type: sistema/adr
---

# ADR-028 — Canal de atualização declarado pelo worker

Status: decidido · Data: 2026-10-31 · Voltar para [[decisoes]].

## Contexto

Uma release só com `linux_signature` (ADR-025) era anunciada a qualquer worker com `can_update`. Um worker AI Cluster OS que virasse canário recusava (ele confere a assinatura de imagem) e o rollout pausava em 10 min. O controller não sabia o canal de cada worker: o identity só dizia que o worker sabia atualizar.

## Decisão

1. **O worker declara o canal.** `NodeIdentity.update_kind` (campo 8): `os` (AI Cluster OS, confere `signature`) ou `linux` (Linux comum, confere `linux_signature`). Vem de `AI_CLUSTER_UPDATE_KIND`, como já era. Vazio é worker antigo e vale como `os`.
2. **O controller só leva a release a quem ela atende.** Um worker entra no rollout e vira canário só se declarou `can_update` e o canal dele é um que a release traz (`signature` para `os`, `linux_signature` para `linux`). Release só-Linux não passa por worker de imagem; release só de imagem não passa por worker Linux. Release com as duas vale para os dois.
3. **Canal desconhecido não atualiza.** Um `update_kind` fora de `os`/`linux` deixa o nó fora do rollout.

## Consequências

- Cluster misto: uma release só-Linux nunca tem um worker AI Cluster OS como canário, e o pausa de 10 min não acontece por esse motivo.
- Worker Linux antigo, que ainda não envia o campo, é tratado como `os` até ser atualizado. Uma release só-Linux não o alcança por esse caminho; uma só-imagem pode alcançá-lo, e ele recusa e pausa o rollout. Atualize esses workers uma vez, pelo `aic update apply`, antes de publicar release de imagem.
- Não há canal separado por worker (ADR-025 descarta): é um filtro sobre a mesma máquina de estados do rollout.

## Alternativas descartadas

- **Campo `bool linux_update`:** serve hoje, mas não deixa nomear o canal se surgir um terceiro. `string` com dois valores conhecidos é o mesmo custo.
- **Controller deduzir o canal de `os_version`:** frágil, depende do texto do `/etc/os-release`.
