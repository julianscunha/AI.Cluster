---
title: "ADR-030 — Wi-Fi no AI Cluster OS"
aliases: ["ADR-030", "Wi-Fi", "wpa_supplicant", "ath9k_htc", "TL-WN821N"]
tags: [sistema/adr, subsistema/plataforma]
type: sistema/adr
---

# ADR-030 — Wi-Fi no AI Cluster OS

Status: decidido · Data: 2026-11-01 · Voltar para [[decisoes]].

## Contexto

Celulares só terão Wi-Fi, e o Orange Pi PC não tem Wi-Fi na placa. Hoje a imagem não traz `wpa_supplicant`, `iw` nem firmware (no x86 só `linux-firmware-rtl_nic`), e o `setup` só configura Ethernet por DHCP ([[ai-cluster-os]], [[adr-017-instalacao-a-b-e-boot-de-rede]]).

## Decisão (proposta)

1. **Adaptador de referência:** TP-Link TL-WN821N (USB `0cf3:7015`, Atheros AR9287), driver `ath9k_htc`, firmware `htc_7010.fw`. O kernel 4.19.20-sunxi do Pi tem `cfg80211` e `mac80211` como módulo, mas não `ath9k_htc` nem `/lib/firmware`: o aparelho aparece no USB e fica sem `wlan0`. O kernel **não é recompilado nem trocado** (rota A): o `.config` do próprio `zImage` já tem `ATH9K_HTC=m`, e o que faltava era o rootfs Alpine ter `/lib/modules`. `build-kernel-wifi.sh` compila só os módulos (`rfkill`, `cfg80211`, `mac80211`, `ath`, `ath9k_hw`, `ath9k_common`, `ath9k_htc`) com vermagic `4.19.20-sunxi SMP mod_unload ARMv7 thumb2 p2v8`, igual ao do `zImage`, e pega o firmware; `build-rootfs.sh` os copia para `/lib` do rootfs (Dockerfile, `depmod -b / 4.19.20-sunxi`, `ath9k_htc` em `/etc/modules`; sem a saída do script a imagem sai sem Wi-Fi, com aviso). O `zImage` fica fixo na FAT `AICBOOT`, fora dos slots, então módulos e firmware entram por atualização assinada, sem regravar o cartão.
2. **Firmware no x86:** pacotes `linux-firmware-*` do Alpine por chipset, instalados só para as famílias suportadas (começa pelo pacote do `ath9k_htc`, se existir; `ath10k` e regdb no Lote 8), nunca o `linux-firmware` inteiro, por causa do tamanho da imagem (pouco disco). Chipset sem firmware na imagem: a interface não sobe e o console diz qual falta.
3. **Credencial:** SSID e senha (WPA2-PSK ou WPA3-SAE) são coletados no convite (`aic worker join`/`setup`) ou na tela de instalação (`aic-wait`/`aic-install.sh`), nunca por argumento de linha de comando. Ficam em `/data/wifi/wpa_supplicant.conf` (0600, root), que sobrevive ao A/B. Nunca aparecem em log, console, `ps`, evento, métrica nem no resumo enviado ao controller (vai só "Wi-Fi configurado" e o SSID). A senha entra por stdin ou por arquivo 0600.
4. **Pilha:** `wpa_supplicant` + `iw` + `networking` do busybox/OpenRC, sem NetworkManager. O país (`iw reg`) é configurável.
5. **Prioridade cabo > Wi-Fi:** com cabo conectado, ele leva a rota padrão (métrica menor); se o cabo cai, o Wi-Fi assume e o worker segue; quando o cabo volta, ele retoma. `node_id` e certificado do túnel não mudam com a interface; o IP muda, e vale a reconexão do worker já existente.
6. **Papel do `setup`:** configura a rede (lista redes com `iw scan`, pede a senha, grava a credencial e valida com um DHCP de teste) e só segue se houver rota para o controller; falha deixa o cabo como está. Não instala firmware: isso é da imagem.
7. **Túnel mTLS e cache de tensores:** nenhuma mudança de protocolo agora. O plano passa a descontar worker em Wi-Fi, mas o desconto sai de medição (cálculo x sincronização por token, Lote 2; vazão e atraso do Pi em Wi-Fi, Lote 9), não de constante chutada. Até lá o worker declara o enlace (`link: wifi`) e recebe peso menor. Ajuste do cache/túnel para enlace instável só depois dos números.
8. **Tela:** subir o Wi-Fi ou trocar de interface não redesenha o console nem pisca a tela; o console atualiza só as linhas que mudaram (sem `clear`).

## Alternativas rejeitadas

- **NetworkManager ou `iwd`:** dependências e RAM a mais para o alvo de pouca memória; `wpa_supplicant` já cobre WPA2/WPA3.
- **Credencial em argumento, variável de ambiente ou no `worker.env`:** aparece em `ps` e em logs.
- **Firmware completo (`linux-firmware`):** centenas de MB na imagem.
- **Wi-Fi como caminho preferido:** vazão e latência piores que o cabo; o Pi a 100 Mb/s já atrasa 25 a 33%.
- **Outro adaptador de referência:** o TL-WN821N é o que o laboratório tem e o `ath9k_htc` é driver livre.

## Consequências

- A imagem ARMv7 ganha os módulos e o firmware (~1,9 MiB), sem mexer no kernel; vai no Lote 4 (um só rebuild).
- O cluster passa a ter enlaces heterogêneos: o plano precisa do desconto, e o teste de reconexão após queda do AP entra no Lote 9.
- A credencial de Wi-Fi é um segredo novo em `/data`: entra no inventário de [[seguranca]].

## Testes em hardware (não feitos)

Orange Pi PC com o TL-WN821N: `wlan0` aparece; associação WPA2 e WPA3; queda e volta do AP (reconexão em tempo limitado); cabo desconectado e reconectado no meio de uma inferência; vazão e atraso em Wi-Fi contra cabo; credencial ausente de `ps`, logs e console; tela sem piscar na troca de interface. Pendência: [[pendencias]].

## Implementação parcial (sem hardware)

Feito e testado por script (`code/tests/os-wifi-overlay.sh`): `wpa_supplicant`, `iw` e `wireless-regdb` nas duas imagens; firmware `linux-firmware-ath9k_htc` (140 KiB) no x86; `/usr/libexec/aic/wifi` (`aic worker wifi set|status|scan|clear`, grava `/data/wifi/wpa_supplicant.conf` 0600 com WPA2-PSK e WPA3-SAE, senha só por stdin); `wlan0` em `/etc/network/interfaces` com métrica 200 (cabo 0); `aic-wifi`/`netwatch` (refaz o DHCP do cabo quando volta, sobe o `wlan0` com credencial, grava o motivo em `/run/aic-wifi.status`); campo `link` no `CapabilityReport` (`wifi` quando a rota padrão sai por interface sem fio) e o plano multiplica a taxa de transferência por 0,5 (constante provisória até o Lote 9). **Kernel do Pi:** o `zImage` vem do sistema Armbian da própria placa (não é construído neste repositório; o rootfs Alpine não tem `/lib/modules`), então `ath9k_htc` exige o kernel 4.19.20-sunxi recompilado (fontes e `.config` do Armbian) ou outro kernel: não feito; ver [[pendencias]].

