---
title: "F19 — Rollout de release, assinatura e attestation"
aliases: ["rollout", "SetRelease", "canário", "assinatura de imagem", "attestation"]
tags: [implementacao/fase/19, implementacao/fatia, implementacao/estado/parcial]
type: implementacao/fatia
status: parcial
superado: false
fase: "[[fase-19-hardening-de-producao]]"
sistema: ["[[ai-cluster-os]]", "[[adr-017-instalacao-a-b-e-boot-de-rede]]", "[[adr-018-release-assinada-e-medida-do-slot]]"]
evidencia: ["code/crates/controller/src/rollout.rs", "code/os/image/overlay/usr/sbin/aic-updater", "code/os/image/sign-release.sh"]
---
# F19 — Rollout de release, assinatura e attestation

- [ ] Superado

## Objetivo

O operador diz qual versão os nós devem rodar; o controller a anuncia, um canário atualiza primeiro e os demais só depois que ele reporta a versão nova com a medida certa. Em imagem A/B, atualizar é gravar o slot inativo; o rollback já é do u-boot ([[adr-017-instalacao-a-b-e-boot-de-rede]]). A release é **assinada** pelo operador e o nó confere antes de gravar; a **medida** do slot em execução volta ao controller como attestation de software ([[adr-018-release-assinada-e-medida-do-slot]]).

## O que existe

**Operador**
- `release-keys.sh [nome]` gera um par Ed25519 em `~/.aic` (a privada nunca sai da máquina; as públicas entram na imagem em `/etc/aic-release.d/`). Rotação: `release-keys.sh nova`; imagem de transição com `AIC_PUBS="~/.aic/release.pub ~/.aic/nova.pub"` assinada com a antiga; a seguinte só com a nova, assinada com a nova (`sign-release.sh … <placa> ~/.aic/nova.key`).
- `sign-release.sh <versão> <rootfs.gz> <rootfs.ext4>` calcula o SHA-256 do `.gz` e da imagem ext4, assina `aic-release-v1`, placa, versão, SHA-256 do gz e SHA-256 do ext4 (uma linha cada) e imprime os argumentos de `ai-cluster rollout <versão> --url … --sha256 … --image-sha256 … --signature …` (`--cancel` encerra). O URL fica **fora** da assinatura (trocar de espelho não exige assinar de novo).

**Controller** (`rollout.rs`, `service.rs`)
- `SetRelease` (token de operador, auditado) valida o formato (sem usuário nem query no URL, hexadecimais, base64 de 64 bytes) e guarda a release em memória e, com `AI_CLUSTER_RELEASE_STATE`, num arquivo de uma linha. Ele **não** verifica a assinatura: só repassa.
- O heartbeat devolve `release` a quem roda outra versão. Só entram no rollout os nós que declaram `can_update`; o primeiro deles a bater vira canário e os outros esperam o canário reportar a versão desejada **e** a medida que bate com a assinada. Com `AI_CLUSTER_ROLLOUT_WAVE=N` só N nós atualizam ao mesmo tempo depois do canário (0 ou ausente = todos); a vaga volta quando o nó reporta a versão ou passados 10 min. Canário que não chega em 10 min pausa o rollout (evento `rollout`) até novo `SetRelease`.
- `ListNodes` calcula `attested` na leitura (versão igual e medida igual à da release assinada); `ai-cluster nodes` mostra `VERSÃO` e `MEDIDA` ("confere" quando a medida bate com a assinada; é o que o nó reporta, não prova).

**Worker**
- Reporta a versão da imagem (`/etc/aic-release`, gravada no build com `AIC_RELEASE`) e a medida do slot (`/run/aic-measure`) em `NodeIdentity`. Com `AI_CLUSTER_UPDATE_CMD`, ao receber release nova valida os caracteres, drena (como no SIGTERM), faz `leave_node` e roda o comando sem shell. A versão tentada fica em `/data/state/update-tried`: se o nó volta por rollback, não repete. Depois do pedido, espera até 20 min pelo reinício; se o updater recusa, lê o motivo em `update-result` e volta ao cluster.

**Imagem** (`aic-update-request`, `aic-updater`, `aic-measure`)
- `aic-update-request` (usuário do worker) deixa o pedido em `/data/state`; o `aic-updater` (root) revalida, **verifica a assinatura** com a chave da imagem (placa em `/etc/aic-board`), **recusa versão que não seja mais nova** (`sort -V`), só baixa de URL no host do controller, adia enquanto `aic_try` está pendente, exige espaço em `/data`, confere SHA-256 e gzip, grava o slot inativo, **relê o slot e compara com a medida assinada** e só então marca `aic_try` e reinicia.
- `aic-measure` (antes do worker) calcula o SHA-256 da partição inteira do slot ativo; o `mke2fs` usa `^has_journal` e o fstab passno `0` para o hash ser estável.

## Evidência (Orange Pi PC, armv7)

- Release `4.4` assinada com o nó na `4.3` (slot B → A): `atualização pedida`, drenou, baixou por HTTP, gravou o slot A, releu e conferiu a medida, reiniciou em teste, o worker registrou e ficou `aic_slot=A`; `/run/aic-measure` igual ao SHA-256 da imagem ext4 assinada; `ai-cluster nodes` com `4.4` e `MEDIDA confere`.
- Assinatura adulterada (1 caractere): `assinatura inválida … ignorado`, nenhum download, slot e `aic_try` intactos, nó de volta ao cluster.
- Revisão de segurança aplicada e repetida no Pi (4.3 → adulterada recusada → 4.4): mensagem e assinatura num diretório do root (0700), `update-result` gravado sem seguir link simbólico, download em diretório só do root, versão só com números e pontos, recusa se a versão instalada é ilegível, canário só conta com medida igual à assinada.
- Rotação de chave: `5.0` (duas públicas, assinada com a antiga) e `5.1` (só a nova, assinada com a nova) instaladas; depois `5.2` assinada com a chave antiga: `assinatura inválida … ignorado`, nó em `5.1` intacto.
- Versão menor assinada (`2.5` sobre `3.0`): `versão 2.5 não é mais nova que 3.0; recusada`.
- Testes unitários: validação (inclui assinatura), canário, canário com medida errada, nó sem `can_update`, pausa, persistência, heartbeat anunciando só a quem difere, `accept_update`.
- A medida do ext4 com journal **não** era estável (um bloco do journal mudava depois do mount ro): por isso `^has_journal`.

## Achados no caminho

- O `sed` do gateway no `aic-slot-commit` gravava o byte 0x01 em vez de `\1`: o gateway saía sempre vazio. Corrigido.
- O busybox `watchdog` sem opções alimenta a cada 30 s e o `sunxi-wdt` reinicia em 16 s: a placa reiniciava a cada ~26 s depois de qualquer boot completo. Agora `-t 5 -T 15`.
- `build-armv7.sh` sem `TARGET=armv7-unknown-linux-musleabihf` gera o binário glibc; a imagem Alpine copiava o musl antigo. Confira `strings` antes de gravar.
- Sem `can_update`, o canário era o primeiro nó que batesse (o x86 do lab, que não atualiza) e o rollout travava. Agora só nós que declaram a capacidade participam.
- O `openssl` nativo do Windows não lê caminhos `/d/…`: o `sign-release.sh` converte com `cygpath`.

## Limites

> [!warning] Ainda não é produção
> - **Rotação só por imagem:** não há lista de revogação; uma chave vazada vale até a imagem que a retira chegar ao nó. Quem tem a chave privada instala o que quiser em todos os nós. Guarde-a fora do repositório e do controller.
> - **A medida não é attestation forte:** é software, reportada pelo próprio nó. Detecta slot corrompido ou errado e gravação ruim, não um nó comprometido (que mente). Integridade de verdade exige dm-verity com root hash assinado e/ou secure boot/TPM, que a placa não tem.
> - **Rollback legítimo precisa de versão maior** (a ordem é numérica; não há downgrade).
> - O `wget` da imagem só faz http e segue redirecionamentos (o SHA-256 e a assinatura limitam o dano); a marca `update-tried` guarda uma versão só.
> - O canário é o primeiro nó apto; as ondas (`AI_CLUSTER_ROLLOUT_WAVE`) só têm teste unitário, sem teste com mais de um nó atualizável em hardware.
> - Um controller malicioso pode mandar uma release com assinatura só no formato correto e segurar o worker fora do cluster até 20 min por tentativa (DoS de disponibilidade, sem instalar nada).
> - A medida lê 512 MiB no boot (~25–40 s no SD) antes do worker subir.
