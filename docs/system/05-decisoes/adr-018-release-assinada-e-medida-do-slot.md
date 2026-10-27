---
title: "ADR-018 — Release assinada pelo operador e medida do slot"
aliases: ["ADR-018", "assinatura de release", "attestation de software"]
tags: [sistema/adr, subsistema/ai-cluster-os, subsistema/seguranca]
type: sistema/adr
---

# ADR-018 — Release assinada pelo operador e medida do slot

Status: decidido · Data: 2026-10-26 · Voltar para [[decisoes]].

## Contexto

O rollout pelo controller manda os nós gravarem um artefato. Se o SHA-256 e o URL viessem só do controller, quem tem o token de operador, ou um atacante no caminho de um download http, instalaria o que quisesse com privilégio de root. A placa de referência não tem TPM nem secure boot.

## Decisão

1. **A release é assinada com Ed25519 pelo operador;** o nó verifica antes de gravar. A mensagem é `aic-release-v1`, placa, versão, SHA-256 do `.gz` e SHA-256 da imagem ext4. O URL fica fora (o `.gz` é autenticado pela soma). As chaves públicas são parte da imagem (`/etc/aic-release.d/*.pub`, raiz somente leitura; vale a assinatura que qualquer uma confere); a privada fica na máquina do operador. O controller só repassa: quem verifica é o `aic-updater` (root, no nó), nunca o worker nem o controller.
2. **O nó recusa versão que não seja mais nova** que a instalada. A assinatura cobre a versão, então o replay de uma release antiga assinada cai aqui. Rollback legítimo é uma versão nova.
3. **O updater relê o slot depois de gravar e compara com a medida assinada** antes de marcar o slot em teste.
4. **Attestation de software por medida:** o `aic-measure` calcula o SHA-256 da partição do slot ativo; o worker reporta em `NodeIdentity.image_sha256`; o controller marca `attested` quando versão e medida batem com a release assinada, e o canário só conta como concluído com a medida certa. Os nós declaram `can_update`; só eles entram no rollout.
5. **Rotação e revogação por imagem:** a imagem de transição leva a pública antiga e a nova (`AIC_PUBS`) e é assinada com a antiga; a seguinte leva só a nova e passa a recusar a antiga. Não há revogação sem uma imagem nova; chave vazada antes da transição segue valendo nos nós que ainda não a receberam.
6. **Fora de escopo, por decisão:** dm-verity, secure boot/TPM, assinatura do boot (u-boot).

## Consequências

- Um controller ou worker comprometido, ou um MITM no download, não instala uma imagem que o operador não assinou. Quem tem a chave privada continua podendo tudo.
- A medida **não** protege contra um nó comprometido, que pode mentir. Ela detecta slot corrompido, gravação ruim e slot errado. `attested` (no CLI, `MEDIDA confere`) não é prova de integridade. O caminho para integridade real é dm-verity com root hash assinado e hardware com raiz de confiança.
- A imagem ext4 sai sem journal (`^has_journal`) e o fstab sem fsck (passno 0): a raiz é somente leitura e o hash precisa ser estável entre boots. Com journal, a montagem somente leitura altera um bloco e muda o hash.
- O boot espera a medida (cerca de 25 a 40 s num cartão SD) antes de o worker subir; o `aic-slot-commit` tem prazo de 300 s e absorve essa espera.
- Trocar a chave são dois rollouts: transição com as duas chaves, depois só a nova. Nós offline na troca precisam ver a transição antes da segunda.

## Alternativas

- Confiar no SHA-256 e no URL vindos do controller: descartada pelo risco descrito no contexto.

## Limites

- Revogação sem nova imagem (lista de revogação) não existe.
- O `wget` do updater usa http; https é pendente.
- dm-verity, secure boot e TPM ficam fora desta decisão.
- Uma imagem suporta uma placa só (`/etc/aic-board`).
