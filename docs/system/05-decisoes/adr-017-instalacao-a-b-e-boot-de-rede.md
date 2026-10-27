---
title: "ADR-017 — Instalação A/B imutável; boot de rede como instalador e recuperação"
aliases: ["ADR-017", "A/B", "reimage"]
tags: [sistema/adr, subsistema/ai-cluster-os]
type: sistema/adr
---

# ADR-017 — Instalação A/B imutável; boot de rede como instalador e recuperação

Status: decidido · Data: 2026-10-26 · Voltar para [[decisoes]].

## Contexto

O worker sobe de um Linux instalado à mão. Para reimage e atualização sem intervenção é preciso uma imagem de sistema que o cluster controle. Três modos foram considerados: instalar a imagem no disco (A/B), rodar um agente num sistema existente e boot de rede sem disco. Mais modos abrem mais variáveis; o produto quer comportamento previsível e hardware suportado por uma matriz de placas.

## Decisão

1. **O caminho oficial é a instalação A/B.** Imagem Alpine imutável (raiz somente leitura), dois slots de sistema (A e B), uma partição de boot FAT e uma de dados (`/data`: identidade do nó, credencial, chaves do host SSH, modelos). Trocar de versão é gravar o outro slot e reiniciar nele.
2. **O boot de rede é só instalador e recuperação,** nunca modo de operação: o u-boot baixa kernel, initramfs e dtb por TFTP, sobe em RAM e o operador grava a imagem (`install-disk.py`). Rodar sem disco e o agente sobre sistema existente ficam como extensões futuras, fora do produto agora.
3. **O estado do A/B mora num setor cru do cartão** (LBA 2016, na folga antes da primeira partição), nunca num arquivo da FAT: o `fatwrite` do u-boot 2018.11 corrompeu arquivos vizinhos. O u-boot lê com `mmc read` e escreve com `mmc write`; o Linux grava com `dd`. A FAT é somente leitura para o u-boot.
4. **Confirmação do slot pelo serviço, não pelo boot:** um slot em teste (`aic_try`) só vira definitivo quando o worker se registra no controller (`aic-slot-commit`). Duas tentativas que falham voltam ao slot confirmado; seis boots sem confirmação levam à recuperação pela rede. Um arquivo `recovery` na FAT força a recuperação.
5. **As definições da recuperação (servidor TFTP, controller, token) vão dentro do `boot.scr`** que o instalador gera sem `mkimage`; `env import` e `source` de um segundo arquivo não aplicaram as variáveis no u-boot da placa de referência.
6. **O instalador só apaga um cartão desmontado, a partir do ambiente em RAM (raiz `rootfs`), e exige `--yes`.**

## Consequências

- Hoje a instalação A/B vale para armv7 (32 bits): boot do cartão, nó pronto, atualização para o slot B com confirmação, rollback automático de um slot B quebrado, recuperação pela rede quando A e B falham, e reinstalação dos slots.
- **Sem assinatura de boot:** o u-boot não verifica o que baixa. Mitigações: o TFTP só atende origens listadas em `--allow` e limita 4 transferências simultâneas e 30 retransmissões; há limite de boots por IP; nenhum segredo na imagem de rede; nó sujeito a aprovação; VLAN isolada recomendada. Um atacante no mesmo segmento que forje o IP ainda pode servir uma imagem.
- O `boot.scr` de cada nó contém o join token em claro (a FAT é legível): trate o cartão como segredo e use tokens de join de uso restrito.
- Arquivos de modelo em RAM no modo sem disco consumiriam memória do worker: um dos motivos de esse modo não ser o oficial.
- Comportamento defensivo: sem recuperação disponível, o u-boot tenta o slot confirmado em vez de resetar para sempre; o `aic-slot-commit` só confirma um slot sem controller se o gateway responde, senão reinicia e o rollback age; o instalador valida o IP do servidor, confere o status do `gunzip` e grava `worker.env` com umask 077; `env import` do estado aceita só as quatro variáveis `aic_*`.

## Alternativas

- Instalação em disco com agente sobre o Linux existente: descartada pelo requisito de comportamento previsível.
- Boot de rede sem disco como modo de operação: os arquivos de modelo ocupariam a memória do worker.

## Limites

- A imagem armv7 foi a primeira; o x86_64 UEFI veio com GRUB ([[adr-019-imagem-x86-uefi-com-grub]]). ARM64 exige instalador e bootloader próprios.
- O u-boot não verifica assinatura do boot. A release é assinada e conferida no nó ([[adr-018-release-assinada-e-medida-do-slot]]).
- O token de recuperação (`AI_CLUSTER_RECOVERY_TOKEN`) só registra, dá heartbeat e reporta saúde, e o worker de recuperação não pede trabalho (`AI_CLUSTER_PRESENCE_ONLY`). O nó de recuperação tem de se chamar `<nó>-rec` (o controller recusa outro id com esse token, e não entrega a ele a cópia do estado nem a release). Vazar o `boot.scr` não dá o token de join. Não é de uso único nem expira: trocá-lo exige regravar o `boot.scr` dos nós.
- O instalador aceita a chave de host do initramfs sem verificação (`AutoAddPolicy`); preferir chave pública é planejado.
- Sem checksum do setor de estado. O arquivo `recovery` força a recuperação até alguém removê-lo, porque o u-boot não escreve na FAT.
- O servidor de boot é um script Python com uma thread por transferência; a versão como serviço com retentativas é planejada ([[provisionamento]]).
