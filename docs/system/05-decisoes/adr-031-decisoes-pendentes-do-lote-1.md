---
title: "ADR-031 — Decisões pendentes do Lote 1"
aliases: ["ADR-031", "trust-listed", "origem do pino", "revogação da chave de Linux", "downgrade do rollout"]
tags: [sistema/adr, subsistema/seguranca, subsistema/plataforma]
type: sistema/adr
---

# ADR-031 — Decisões pendentes do Lote 1

Status: decidido · Data: 2026-11-01 · Voltar para [[decisoes]].

Reúne as decisões abertas do Lote 1 de [[pendencias]], cada uma com recomendação. Nenhuma está implementada. Aprovar item a item.

## 1. Base Linux ([[adr-001-base-linux]])

Contexto: o ADR-001 segue "proposto" e diz que a distribuição está aberta, mas as imagens ARMv7 e x86 já usam Alpine 3.20 com OpenRC ([[adr-017-instalacao-a-b-e-boot-de-rede]], [[adr-019-imagem-x86-uefi-com-grub]]).
Opções: (a) fixar Alpine; (b) Debian/Ubuntu minimal; (c) Buildroot/Yocto.
**Recomendação: (a), fechar o ADR-001 como decidido com Alpine + OpenRC.** Motivo: é o que já roda, é pequeno (alvo de pouca RAM e disco) e tem a raiz somente leitura funcionando. Reabrir só se um acelerador exigir glibc (Lote 10).

## 2. ADR da fase F03

Contexto: a pendência pede "ver se alguma decisão pede ADR" na F03 (capability, execução).
Opções: (a) novo ADR da F03; (b) nenhum, dar a pendência por fechada.
**Recomendação: (b).** Motivo: as decisões da F03 já estão cobertas ([[adr-022-fluxo-de-modelos-e-motor-hibrido]], [[adr-020-niveis-de-confianca-do-no]]); um ADR sem decisão nova é ruído. Ao aprovar, remover a linha de pendencias; decisão nova abre ADR no ato.

## 3. Atalho `--trust-listed`

Contexto: aprova a impressão que o controller mostra, sem comparar com a do worker; anula a proteção contra o id tomado ([[seguranca]]). Usado em scripts de teste e no bootstrap manual.
Opções: (a) manter como está; (b) manter só com habilitação explícita no controller (variável de ambiente), com evento de auditoria e aviso; (c) remover e fazer os scripts passarem a impressão.
**Recomendação: (b) agora, caminhando para (c).** Motivo: os scripts E2E dependem dele; exigir habilitação explícita e registrar evento tira o atalho do uso casual sem quebrar o laboratório. O bootstrap manual passa a usar a impressão que o worker imprime.

## 4. Gateway atrás de NAT

Contexto: o id do gateway é amarrado ao IP; atrás de NAT todos parecem um só e quem troca de IP espera 45 s. O limite por IP (`AI_CLUSTER_MAX_GATEWAYS_PER_HOST`) existe, mas o token de inferência é compartilhado.
Opções: (a) manter o IP e o limite; (b) segredo por gateway, gerado no primeiro registro e apresentado nos seguintes, no lugar do IP; (c) mTLS com certificado por gateway.
**Recomendação: (b).** Motivo: resolve NAT e id tomado sem nova PKI; (c) é mais forte, mas só compensa quando houver rotação da CA. Manter o limite por IP como segunda barreira.

## 5. Origem do pino da chave de Linux

Contexto: a chave pinada vem da origem da instalação (confiança no primeiro uso, [[adr-025-gatilho-de-atualizacao-assinada-linux]]); quem controla o download controla a chave.
Opções: (a) manter confiança no primeiro uso; (b) mostrar a impressão da chave no convite e o instalador comparar antes de pinar; (c) embutir a chave num instalador assinado por outra raiz.
**Recomendação: (b).** Motivo: a impressão viaja por outro canal (o convite que o operador copia do controller) e não exige raiz nova. Em modo não interativo, o instalador exige a impressão como parâmetro.

## 6. Chave que assina o `SHA256SUMS` do instalador

Contexto: hoje o pino do instalador dedicado é o `release.pub` do rollout (chave do AI Cluster OS), mas o controller serve o `SHA256SUMS` assinado com a chave de Linux ([[adr-025-gatilho-de-atualizacao-assinada-linux]]).
Opções: (a) chave do rollout; (b) chave de Linux; (c) terceira chave só do instalador.
**Recomendação: (b).** Motivo: o instalador baixa os binários Linux, que essa chave já assina; usar a do rollout mistura raízes e (c) acrescenta uma chave a guardar. O pino do instalador vira a chave de Linux, conferida pela impressão do item 5.

## 7. Revogação da chave de Linux

Contexto: a rotação sem reinstalar está pronta (ADR-025, item 9). Falta decidir quem revoga, como a lista chega sem depender da chave revogada e se revogar barra o retorno.
Opções: (a) sem lista, só rotação; (b) lista de revogação assinada pela chave nova da rotação, distribuída junto do `SHA256SUMS` e conferida antes de qualquer hash; (c) CRL por canal HTTP separado.
**Recomendação: (b), com a revogação feita pelo operador no controller.** Motivo: a chave comprometida não pode validar a lista que a revoga, mas a chave rotacionada (já pinada) pode; sem canal novo. Revogar barra o retorno: a chave revogada é recusada mesmo com artefato bem assinado. Se a chave pinada está comprometida e não há outra, só reinstalando (limite aceito, a documentar).

## 8. Downgrade do rollout

Contexto: o atualizador recusa versão menor ou igual ([[adr-029-versao-unica-do-produto]]) e a marca guarda uma versão só. Um rollback legítimo hoje exige publicar uma versão maior.
Opções: (a) nunca permitir (roll-forward); (b) permitir só com bandeira assinada na release, limitada a uma versão que o worker já rodou; (c) `--force` local no worker.
**Recomendação: (a) como regra, (b) como exceção.** Motivo: roll-forward mantém a proteção contra ataque de reversão; (c) abriria o ataque a quem tem acesso ao worker. A exceção (b) só entra no código se o primeiro rollback real pedir.

## Consequências

- Aprovados os itens, as linhas de [[pendencias]] viram código (Lote 3) ou somem (itens 1 e 2).
- Os itens 5 a 7 fixam formato de convite, pino e lista de revogação antes de tocar nesses fluxos.
