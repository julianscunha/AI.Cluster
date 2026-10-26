---
title: "F19 — Rollout de release pelo controller"
aliases: ["rollout", "SetRelease", "canário"]
tags: [implementacao/fase/19, implementacao/fatia, implementacao/estado/parcial]
type: implementacao/fatia
status: parcial
superado: false
fase: "[[fase-19-hardening-de-producao]]"
sistema: ["[[ai-cluster-os]]", "[[adr-017-instalacao-a-b-e-boot-de-rede]]"]
evidencia: ["code/crates/controller/src/rollout.rs", "code/os/image/overlay/usr/sbin/aic-updater"]
---
# F19 — Rollout de release pelo controller

- [ ] Superado

## Objetivo

O operador diz qual versão os nós devem rodar; o controller a anuncia, um canário atualiza primeiro e os demais só depois que ele reporta a versão nova. Em imagem A/B, atualizar é gravar o slot inativo; o rollback já é do u-boot (ADR-017).

## O que existe

- `ai-cluster rollout <versão> --url <http(s)> --sha256 <64 hex>` (`--cancel` encerra) chama `SetRelease` (token de operador, auditado). O controller valida (sem usuário nem query no URL) e guarda a release em memória e, com `AI_CLUSTER_RELEASE_STATE`, num arquivo de uma linha.
- O heartbeat devolve `release` só a quem roda outra versão e foi liberado. O primeiro nó a bater vira canário; os outros esperam o canário reportar a versão desejada. Se o canário não chega em 10 min, o rollout pausa (evento `rollout`) até um novo `SetRelease`.
- O worker reporta a versão da imagem (`/etc/aic-release`, gravada no build com `AIC_RELEASE`), não a do crate. Com `AI_CLUSTER_UPDATE_CMD`, ao receber uma release nova ele valida os caracteres, drena (sai do loop como no SIGTERM), faz `leave_node` e roda o comando sem shell. A versão tentada fica em `/data/state/update-tried`: se o nó volta por rollback, não repete.
- Na imagem, o comando é `aic-update-request` (deixa o pedido em `/data/state`) e o serviço root `aic-updater` revalida, baixa para `/data`, confere o SHA-256 e o gzip, grava o slot inativo, marca `aic_try` no setor 2016 e reinicia. O worker roda sem privilégio, por isso a divisão.

## Evidência (Orange Pi PC, armv7)

- Rollout `2.0` com o nó na `1.0` (slot B): `atualização pedida` → drenou (`node_left`) → baixou por HTTP → gravou o slot A → reiniciou → A em teste → worker registrou → `aic_slot=A`, `/etc/aic-release` = 2.0, watchdog ativo.
- Testes unitários: validação da release, canário e pausa, persistência, heartbeat anunciando só a quem difere, `accept_update`.

## Achados

- O `sed` do gateway no `aic-slot-commit` gravava `\1` como byte 0x01 (arquivo commitado com o bug): o gateway saía sempre vazio. Corrigido.
- O busybox `watchdog` sem opções alimenta a cada 30 s e o `sunxi-wdt` reinicia em 16 s: a placa reiniciava a cada ~26 s depois de qualquer boot completo. Agora `-t 5 -T 15` (`WATCHDOG_OPTS` na imagem).
- `build-armv7.sh` sem `TARGET=armv7-unknown-linux-musleabihf` gera o binário glibc; a imagem Alpine copiava o musl antigo. Confira `strings` no binário antes de gravar.

## Limites

> [!warning] Ainda não é produção
> - **Sem assinatura:** o SHA-256 vem do mesmo controller que manda o URL; quem tem o token de operador (ou um MITM em http) instala o que quiser em todos os nós. Assinatura (chave pública na imagem) é o próximo passo. O `wget` da imagem não faz https.
> - **O canário é o primeiro nó que bate,** sem saber se ele consegue atualizar (um nó sem `AI_CLUSTER_UPDATE_CMD` pode ser escolhido e o rollout pausa em 10 min). Falta o nó anunciar essa capacidade.
> - Sem proteção contra downgrade (não há ordem total de versões); quem define é o operador.
> - Um nó que mente a versão só engana o próprio rollout (sem attestation).
> - O `aic-updater` (root) só baixa de URL no mesmo host do controller (`worker.env`), recusa pedido por link simbólico, adia enquanto `aic_try` está pendente e exige espaço em `/data`. Um worker comprometido ainda pode pedir gravação de uma imagem servida por esse host; a assinatura fecha isso. O `wget` segue redirecionamentos e a marca `update-tried` guarda uma versão só.
> - Teste em um nó; a linha de ondas além do canário (N configurável) não existe.
