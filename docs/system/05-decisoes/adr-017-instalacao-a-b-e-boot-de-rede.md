---
title: "ADR-017 — Instalação A/B imutável; boot de rede como instalador e recuperação"
aliases: ["ADR-017", "A/B", "reimage"]
tags: [sistema/adr, subsistema/ai-cluster-os]
type: sistema/adr
status: decidido
implementacao: parcial
fases: ["[[fase-15-ai-cluster-os]]", "[[fase-19-hardening-de-producao]]"]
---

# ADR-017 — Instalação A/B imutável; boot de rede como instalador e recuperação

Data: 2026-10-26 · Voltar para [[decisoes]].

## Contexto

O worker sobe de um Linux que alguém instalou (F15 J3). Para reimage e atualização sem intervenção é preciso uma imagem de sistema que o cluster controle. Discutimos três modos: instalar a imagem no disco (A/B), rodar um agente num sistema existente e boot de rede sem disco. Mais modos abrem mais variáveis; o produto quer comportamento previsível e hardware suportado por matriz (F18 Q1).

## Decisão

1. **O caminho oficial é a instalação A/B.** Imagem Alpine imutável (raiz somente leitura), dois slots de sistema (A e B), uma partição de boot FAT e uma de dados (`/data`: identidade do nó, credencial, chaves do host SSH, modelos). Trocar de versão é gravar o outro slot e reiniciar nele.
2. **O boot de rede é só instalador e recuperação,** nunca modo de operação: o u-boot baixa kernel, initramfs e dtb por TFTP, sobe em RAM e o operador grava a imagem (`install-disk.py`). Rodar sem disco e o agente sobre sistema existente ficam como extensões futuras, fora do produto agora.
3. **O estado do A/B mora num setor cru do cartão** (LBA 2016, na folga antes da primeira partição), nunca num arquivo da FAT: o `fatwrite` do u-boot 2018.11 corrompeu arquivos vizinhos (observado). O u-boot lê com `mmc read` e escreve com `mmc write`; o Linux grava com `dd`. A FAT é somente leitura para o u-boot.
4. **Confirmação do slot pelo serviço, não pelo boot:** um slot em teste (`aic_try`) só vira definitivo quando o worker se registra no controller (`aic-slot-commit`). Duas tentativas que falham voltam ao slot confirmado; seis boots sem confirmação levam à recuperação pela rede. Um arquivo `recovery` na FAT força a recuperação.
5. **As definições da recuperação (servidor TFTP, controller, token) vão dentro do `boot.scr`** que o instalador gera sem `mkimage`; `env import` e `source` de um segundo arquivo não aplicaram as variáveis no u-boot do Orange Pi.
6. **O instalador só apaga um cartão desmontado, a partir do ambiente em RAM (raiz `rootfs`), e exige `--yes`.**

## Consequências

- Validado no Orange Pi PC (armv7): boot do cartão, nó READY, atualização para B com confirmação, rollback automático de um B quebrado, recuperação pela rede quando A e B falham, e reinstalação dos slots. Ver [[f15-imagem-ab-e-reimage]].
- **Sem assinatura de boot:** o u-boot não verifica o que baixa. Mitigações: `--allow` por IP no TFTP, limite de boots por IP, nenhum segredo na imagem de rede (o token vai na linha de comando do cartão), nó sujeito a aprovação, e VLAN isolada recomendada. Um atacante no mesmo segmento que forje o IP ainda pode servir uma imagem.
- O `boot.scr` de cada nó contém o join token em claro (a FAT é legível): trate o cartão como segredo e use tokens de join de uso restrito.
- Arquivos de modelo em RAM no modo sem disco consumiriam memória do worker: um dos motivos de esse modo não ser o oficial.
- Fica aberto: ARM64 e x86 (instalador e u-boot/GRUB próprios), assinatura da imagem, rollout automático pelo controller (F19) e o servidor de boot como serviço com retentativas (hoje um script Python de uma thread por transferência).
