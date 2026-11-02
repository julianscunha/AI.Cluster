---
title: "Instalação"
aliases: ["Instalar", "Compilar e instalar"]
tags: [guia, instalacao]
type: guia
---

# Instalação

Volte ao [[guia]] para o índice. Valores de configuração estão em [[configuracao]].

## Requisitos

- Toolchain Rust com a edição 2024 (Rust 1.85 ou mais novo). O `protoc` não é preciso: o build usa um `protoc` vendorizado.
- Docker, para o cluster de desenvolvimento, os testes E2E e o build cruzado para armv7.
- `openssl`, para gerar certificados e chaves de release.

## Instalador guiado (recomendado)

Um script só pergunta o que este computador será e baixa da **última release** apenas o que esse papel precisa:

```bash
curl -fsSL https://github.com/julianscunha/AI.Cluster/releases/latest/download/install.sh | sudo sh
```

Em inglês: acrescente `-s -- --lang en` ao `sh` (ou `AIC_LANG=en`). O idioma vale para o instalador, o `aic` e os scripts do convite, e fica gravado em `/etc/ai-cluster/lang`. Sem isso tudo sai em português. Em modo inglês, `--mode dedicated`, `--kind physical` e `--kind container` também são aceitos.

O download usa HTTPS sempre que a origem é `https://` (a release oficial ou um espelho): `install.sh`, `aic update apply` e o cache de pacotes do convite recusam HTTP e qualquer redirecionamento para HTTP, com TLS 1.2 no mínimo, e a assinatura e o hash do `SHA256SUMS` seguem conferidos. Origem `http://` (lab, `AIC_BASE`) é aceita sem essa trava. O servidor do convite (porta 8099) continua em HTTP: o que o worker baixa dele é conferido pela impressão da chave (`--key-fp`) e pelo hash.

Ele não detecta o hardware: quem dita é você. Só confere se o processador informado é o da máquina, para não instalar um binário que não roda. As perguntas:

1. **Como instalar:** neste Linux, como serviço, ou dedicando o computador ao cluster (veja "Instalação dedicada").
2. **Este computador será:** controller (e, junto, o gateway ou não), gateway noutro computador, ou worker.
3. **Processador:** x86_64, ARM de 32 bits (armv7) ou ARM de 64 bits (ainda sem pacote). E se é físico, máquina virtual ou contêiner.

Os componentes do cluster:

| Componente | Para que serve | Onde roda |
|---|---|---|
| Controller | Registra os computadores, decide onde cada tarefa roda, guarda chaves e modelos. Um por cluster. | x86_64 e armv7 |
| Gateway | Recebe os pedidos de chat (API compatível com a da OpenAI) e divide o modelo entre os workers. Um por cluster. | x86_64 (o `llama-server` só existe para x86_64) |
| Worker | Empresta processador e memória. Quantos quiser. | x86_64 e armv7 |

A ordem é sempre: **primeiro o controller**, depois os outros.

```bash
# 1. no primeiro computador: escolha "Controller" (com o gateway junto, o mais simples)
# 2. ainda nele: abre um convite de 15 min e mostra as linhas de entrada
sudo aic cluster invite   # mostra um código de 8 caracteres
# 3. em cada outro computador, o mesmo instalador, escolhendo "Worker" (ou a linha curta do convite)
curl -fsSL http://<ip-do-servidor>:8099/join-<CÓDIGO> | sudo sh
```

O **código do convite** (8 caracteres, letras maiúsculas e números sem `0/O/1/I`) é a credencial do convite: o instalador o pergunta quando o papel é worker ou gateway (minúsculas valem), e a linha curta o leva no endereço (`/join-<CÓDIGO>`). Sem ele o servidor não entrega o token de join nem assina o certificado do túnel. Como o código é curto, cada endereço IP que errar 30 vezes (arquivo inexistente em `/join-*`, `/gw-*`, `/node-*`, por GET ou HEAD, com o caminho já normalizado, ou `/sign` com código errado) é travado com 429, e há um teto global de 300 chutes; o terminal do convite avisa quando um IP trava, e os outros endereços seguem servidos. O convite expira sozinho no fim da janela (o servidor Python também se encerra, mesmo que o terminal seja fechado à força). O convite continua em HTTP puro, então o código serve contra quem só alcança a porta, não contra quem escuta a rede.

O pacote do worker de cada processador vem do próprio servidor, na mesma versão do cluster: o servidor o baixa da release na primeira vez que abre um convite e o guarda em cache. Os outros computadores não precisam de internet. Cada worker gera a própria chave do túnel RPC; o servidor só assina o pedido de certificado (SAN `aic-worker-<nome do worker>`), e a chave nunca sai do worker. O gateway em outro computador entra pelo mesmo convite (papel gateway no instalador, ou `sudo aic gateway install --server <ip> --code <código>`), mas a pasta de modelos precisa ser a mesma que o controller enxerga (NFS, por exemplo).

Sem perguntas (automação): `sudo sh install.sh --mode linux --role server|controller|gateway|worker --arch x86_64|armv7 --kind fisico|vm|conteiner [--server IP] (--key-fingerprint <impressão> | --trust-origin-key) --yes`. Sem terminal ou com `--yes` o instalador exige a impressão da chave de Linux (32 hex, do convite ou do aviso da release) ou o aceite explícito `--trust-origin-key`. Com `--role controller` em x86_64 o `install.sh` baixa também o pacote do gateway: o `setup` instala `ai-cluster-gateway` e `llama-server` (sem subir o serviço, que só sobe com `--role server` ou noutro computador pelo convite) e o `gateway.env` aponta para um binário que existe. Em ARMv7 não há `llama-server`, e o `gateway.env` não traz `AI_CLUSTER_LLAMA_SERVER_BIN`.

O `install.sh` baixa `VERSION`, `SHA256SUMS`, `SHA256SUMS.sig` e os pacotes `tools`, `controller-linux-<arq>`, `gateway-linux-x86_64` e `worker-linux-<arq>`. Confere a assinatura do `SHA256SUMS` com a chave que vem no `tools` (`release.pub`) antes de usar qualquer outro arquivo, e o SHA-256 de cada pacote. A chave fica gravada em `/etc/ai-cluster/release.pub`, e é com ela que as atualizações futuras são conferidas. `bash code/scripts/package-release.sh` gera esses arquivos (nomes sem versão, para `releases/latest/download/`) e assina o `SHA256SUMS` com `~/.aic/linux.key` (gerada por `bash os/image/release-keys.sh linux`; a privada nunca vai para o repositório). Sobre um Linux existente o desempenho é compartilhado com o que já roda nele e o cluster confia no sistema operacional da máquina; o modo dedicado existe para evitar os dois problemas.

Requisito: **OpenSSL 3 ou mais novo** (`openssl version`). A conferência da assinatura usa Ed25519 bruta (`-rawin`), que o OpenSSL 1.1 não tem; o Debian 12 traz o 3. Sem ele, o instalador e o `aic update` recusam com essa mensagem.

O `install.sh` mostra a **impressão da chave de Linux** (32 hex do SHA-256 de `release.pub`) antes de pinar e a confere com a que você traz de outro canal: `--key-fingerprint` (ou `AIC_KEY_FINGERPRINT`), o aviso da release (`package-release.sh` a imprime) ou, no worker, o convite (`aic cluster invite` mostra a impressão e o comando já leva `--key-fp=`; o script de entrada a recalcula e interrompe se diferir). Com terminal e sem a impressão, ele pergunta; `--trust-origin-key` aceita a chave da origem sem conferir (confiança no primeiro uso, só quando você aceita o risco: quem controla a origem controla o pino). A proteção real vem nas atualizações seguintes: depois de instalado, só uma release assinada pela chave pinada atualiza a máquina.

### Apresentar um computador para instalar o AI Cluster OS

No convite aberto (`sudo aic cluster invite`) o servidor mostra também a linha para quem já tem Linux e quer virar um worker dedicado:

```bash
curl -fsSL http://<ip-do-servidor>:8099/wait | sudo sh
```

O script confere o processador, o firmware e o espaço, baixa só o `aic-wait` do servidor (SHA-256 conferido) e o roda. A tela mostra a **impressão digital**, o MAC, a CPU, a RAM e os discos (um disco removível, como pendrive ou USB, aparece marcado). No servidor: `aic cluster install list` mostra a tabela dos aparelhos; confira a impressão digital com a da tela e rode `aic cluster install approve`. Sem argumentos, ele escolhe o único aparelho pendente e o único disco interno, mostra `vai apagar <disco> <modelo> <tamanho>` e pede confirmação; com vários aparelhos ou discos, pergunta qual. Para outro disco ou outro aparelho, use `aic cluster install approve <impressão> --disk <disco>`. Disco removível só é apagado com `--force-removable`. `--yes` aprova sem perguntar (automação). `aic cluster install reject <impressão>` tira da lista. A porta de espera só aceita aparelhos enquanto o convite está aberto (ou `aic cluster install open`). O reinício para a imagem de espera e a instalação em si ainda não existem ([ADR-023](../system/05-decisoes/adr-023-instalacao-com-aprovacao-do-controller.md)).

### Atualizar este computador (`aic update`)

```bash
sudo aic update status           # versão instalada × disponível
sudo aic update apply            # pergunta antes; --yes não pergunta
sudo aic update apply --force    # reinstala a versão atual (mesmo sem novidade)
sudo aic update apply --version 0.2.4
```

O que cada componente recebe, e o que o controller empurra, está em [[operacao#Atualização de cada componente]]. Em resumo: controller e gateway em Linux, e o worker em Linux, se atualizam com `aic update apply`, sem perder tokens, certificados, workers aprovados nem modelos; o worker em Linux comum recebe a atualização que o controller empurra, assinada com a chave de Linux ([ADR-025](../system/05-decisoes/adr-025-gatilho-de-atualizacao-assinada-linux.md)); o worker em AI Cluster OS é atualizado pela release assinada que o controller empurra (`aic cluster rollout`).

Servidores instalados antes da assinatura (sem `/etc/ai-cluster/release.pub`) recusam `aic update` com a instrução de migração: rode de novo o `install.sh` desta versão, com o mesmo papel, e depois `sudo aic update apply`. Ele grava a chave e não mexe no resto.

O pacote do worker que o convite entrega é o da versão do servidor: depois de atualizar, o próximo `aic cluster invite` baixa o novo. Os workers que já estão no cluster não precisam de nada.

Um servidor instalado **antes** da troca de nomes ainda tem a ferramenta antiga, sem o `aic`. Baixe o `setup` da release (ele vem em `tools.tar.gz`) e rode-o uma vez: `sudo sh /tmp/setup update apply --force`. Depois disso o `aic` passa a existir.

### Mudar um worker (AI Cluster OS) de controller

Num worker que já roda o AI Cluster OS (placa ou PC dedicado) a identidade fica em `/data/worker.env`, gravada na imagem. Para apontá-lo para outro controller sem regravar a imagem, rode **no worker**, com o convite do novo controller aberto (`sudo aic cluster invite` lá):

```bash
aic worker change --server <ip-do-novo-controller>:8099 --code <CÓDIGO>
# se o controller aparece para o worker com outro IP (por exemplo, atrás de NAT):
aic worker change --server <ip>:8099 --code <CÓDIGO> --controller http://<ip-que-o-worker-alcança>:50051
```

O comando mostra o controller de destino e pede confirmação (`S/n`; `--yes`/`AIC_ASSUME_YES=1` dispensa). Depois baixa o token e a CA do convite, gera uma chave nova **no próprio worker**, pede o certificado do túnel, confere tudo e só então troca o `worker.env`, apaga a credencial antiga e reinicia o worker. Se qualquer passo falhar, o worker fica como estava (o estado anterior fica em `/data/.change-backup`). Depois o worker aparece como pendente no novo controller; o convite aberto o aprova sozinho. Só o endereço e o token (sem mexer no certificado do túnel): `aic worker change --controller <url> --token <token>`. Para o token não aparecer no `ps`, use `--token-file <arquivo>` (arquivo só do dono, `chmod 600`).

## Instalação dedicada

Dedicar o computador grava o AI Cluster OS no disco: um sistema só para o cluster, imutável, com slots A/B, volta automática e release assinada ([[adr-017-instalacao-a-b-e-boot-de-rede]], [[adr-019-imagem-x86-uefi-com-grub]]). É o equivalente ao ESXi: desempenho previsível, sem outros serviços competindo.

**Hoje:** só o **worker** tem imagem (armv7 e x86_64 UEFI). Controller e gateway dedicados ainda não estão nas imagens ([[pendencias]]). O instalador guiado já oferece a opção e explica isso.

**Ordem de deploy:** o controller vem primeiro. O worker só confirma o slot novo depois de se registrar no controller, e a imagem já leva o endereço dele e o join token.

**Como a imagem chega ao computador (previsto para o instalador guiado):**
- **Pela rede (boot de rede):** restrito. O usuário informa o **MAC** de cada worker e só esses endereços recebem a imagem; o boot de rede é só instalador e recuperação, nunca modo de operação. Hoje o servidor TFTP já limita as origens por IP (`--allow`), com limite de boots; falta o filtro por MAC.
- **Por pendrive:** quando a rede não tem DHCP ou o computador não é visível nela, o controller entrega um `usb.zip` pelo navegador (passo a passo abaixo). Nenhum dd, Rufus ou terminal no computador novo.

### Hardware e requisitos

| Aparelho | Estado | Observações medidas |
|---|---|---|
| x86_64 UEFI | **Testado** em notebook (i7-8565U, 8 GB de RAM, NVMe, Ethernet 1 Gb/s) | Cabo a 1 Gb/s sem erros. O Wi-Fi do notebook só funciona se o chipset tiver firmware na imagem (hoje: Atheros `ath9k_htc`, pacote `linux-firmware-ath9k_htc`); sem ele use cabo (ADR-030, não testado em hardware). A NIC Realtek funciona: a imagem traz o firmware dela (`linux-firmware-rtl_nic`, `rtl8168h-2.fw`). |
| armv7, Orange Pi PC | **Testado** (cartão SD, Ethernet 100 Mb/s, 1 GB de RAM) | 100 Mb/s é o limite da placa. O SD é lento sob gravação com `fsync` (cerca de 97 ms por gravação de 4 KiB, contra 4,7 ms no NVMe). |
| BIOS legado/CSM, Wi-Fi, Secure Boot, ARM64, GPU/NPU | **Não testados** | A imagem x86 exige UEFI com Secure Boot desligado; não há imagem ARM64; GPU e NPU não são usadas. |

Requisitos mínimos reais:

- **RAM:** a do worker (o processo usa poucos MB) mais a do instalador x86, que mantém a imagem comprimida na RAM durante a gravação (a imagem tem 3 GiB descomprimida). Foi testado com 8 GB. O mínimo de RAM do instalador é **estimado, não medido**.
- **Firmware opcional (fora da imagem):** a imagem x86 inclui só o da NIC Realtek (`linux-firmware-rtl_nic`, poucos MB; tamanho não medido). Vídeo Intel (`linux-firmware-i915`), Wi-Fi Atheros (`linux-firmware-ath10k`) e `wireless-regdb` ficam de fora de propósito, porque cada um soma megabytes (alguns, dezenas) a uma imagem que cabe em disco pequeno; se precisar de algum, acrescente o pacote Alpine ao `apk add` de `os/image/x86/Dockerfile` e reconstrua a imagem. Sem Wi-Fi, use cabo.
- **Watchdog (x86):** o serviço `aic-watchdog` só liga se o `/dev/watchdog` existir (o `iTCO_wdt` o cria em muitos PCs); sem o dispositivo o boot segue sem ele.
- **Disco:** maior que a imagem (3 GiB) mais os dados do worker (modelos e fatias ficam em `/data`, que ocupa o resto do disco). O disco escolhido é **apagado por inteiro**.
- **Rede:** Ethernet com DHCP, alcançando o controller (portas 8099 e 50051 por padrão). Sem DHCP o instalador não sobe a rede.
- **Teclado e monitor:** opcionais. Sem eles, o pendrive entra sozinho em 5 s e a escolha do disco é feita no controller; com eles, você escolhe o disco na tela e usa o console (F2).

### Modos de inicialização

| Modo | Estado | Como é |
|---|---|---|
| Pendrive gerado pelo controller (x86_64 UEFI) | Disponível | Convite (`sudo aic cluster invite`) entrega o `usb.zip`; você o extrai num pendrive FAT32. O GRUB UEFI procura `/aic/grub.cfg` em todos os discos e partições (sem depender de rótulo ou letra), mostra um menu por setas (**Instalar AI Cluster OS**, entra em 5 s; **Reiniciar**) e o instalador pede a escolha do disco na tela. |
| Cartão SD (Orange Pi) | Disponível | Grava-se a imagem no cartão (veja "Imagem do AI Cluster OS (armv7)"). Depois, a atualização é a assinada pelo caminho A/B (`aic cluster rollout`), sem regravar. |
| Rede/PXE | **Não implementado** para x86 | Previsto como instalador e recuperação, nunca modo de operação ([[adr-017-instalacao-a-b-e-boot-de-rede]]). Hoje só existe o boot de rede do armv7 (u-boot e TFTP, `os/netboot/`); PXE em x86 aparece em [[pendencias]] (J1) e a restrição por MAC também. |

### Etapas de ponta a ponta (x86_64)

1. **Abrir o convite**, no controller: `sudo aic cluster invite` (ou só a porta de espera, `aic cluster install open`). Ele mostra o endereço e o SHA-256 do `usb.zip`.
2. **Gerar o pendrive:** baixe o `usb.zip`, confira o SHA-256, formate o pendrive em FAT32 e extraia o zip na raiz (passo a passo em "Pendrive pelo navegador").
3. **Dar boot no aparelho** pelo pendrive em modo UEFI (Secure Boot desligado). A tela mostra a **impressão digital**, o IP e o MAC.
4. **Conferir:** no controller, `aic cluster install list` mostra os aparelhos; a impressão digital deve ser igual à da tela.
5. **Aprovar:** `aic cluster install approve` (ou `... approve <impressão> --disk <disco>`). Mostra `vai apagar <disco> <modelo> <tamanho>` e pede confirmação.
6. **Instalação:** o aparelho confere a assinatura e o SHA-256 da imagem, mostra 10 s para desistir, apaga o disco, grava a imagem e pede para tirar o pendrive. Depois **reinicia** pelo disco.
7. **Aprovar o worker:** ele se registra como pendente. No controller, `aic cluster nodes` mostra a `IMPRESSÃO`; aprove com `aic cluster node approve <id> --fingerprint <impressão>`.
8. **Conferir no console** do aparelho (monitor): o painel "Estado do worker" mostra Registro, Impressão e Aprovação; **F2** abre o menu, e "Saúde do computador" mostra CPU, memória, temperatura, disco e rede.
9. **SSH:** `ssh root@<ip-do-worker>` com a chave do operador (a que estava em `authorized_keys` de quem rodou o convite). Defina a senha do console com `aic worker passwd`.
10. **Atualizar depois:** `aic cluster rollout <versão> ... --url <endereço>` com a release assinada; o worker grava o outro slot (A/B), reinicia e volta sozinho ao slot anterior se não se registrar ([[operacao]]).

### Solução de problemas

| Sintoma | Causa e saída |
|---|---|
| Prompt `grub>` | O pendrive não foi extraído na raiz (ou o zip está incompleto). Extraia o `usb.zip` na raiz, em FAT32, e confira que existem `EFI` e `aic`. Se aparecer o menu "Tentar de novo", é o mesmo caso. |
| Tela sem texto | Tente outra saída de vídeo ou outro modo de boot UEFI; a imagem não traz firmware de vídeo, e sem ele a tela pode ficar preta. Acompanhe pelo controller (`aic cluster install list`). |
| Sem DHCP | O instalador não alcança o controller. Confira o cabo, o DHCP da rede e que o controller é alcançável a partir dela. Wi-Fi: a imagem traz `wpa_supplicant` e `iw`; grave a credencial no aparelho com `aic worker wifi set <SSID>` (a senha vem pelo stdin ou é pedida, nunca na linha de comando; fica em `/data/wifi/wpa_supplicant.conf`, 0600) e confira com `aic worker wifi status`. O cabo tem prioridade; sem ele o Wi-Fi assume. Na imagem armv7 o adaptador USB ainda não tem driver no kernel: use cabo. A coleta no convite/instalador e o teste em hardware estão em [[pendencias]]. |
| "assinatura do SHA256SUMS não confere" | A chave pinada no instalador não é a que assinou a release. Reconstrua o instalador com a chave certa (`build-installer.sh`) ou use o pendrive do mesmo controller. Nada é apagado antes dessa conferência. |
| Hora errada e certificado | Sem RTC ou com RTC atrasado, o certificado pode parecer expirado. O worker corrige a hora pela rede (DNS e chrony); confira `date` no aparelho e a rede. Veja "Contêiner do worker" e `certificate expired`. |
| F2/F12 não respondem em notebook | Muitos notebooks exigem a tecla Fn junto (Fn+F12 para o menu de boot, Fn+F2 para a BIOS ou o console). |
| Disco removível recusado | Pendrives e USBs só são apagados com `--force-removable` no `approve`; se for a única opção, confira se é mesmo o disco que quer. |
| Convite expirado ou porta fechada | A porta de espera fecha sozinha (padrão 15 min). Abra de novo com `aic cluster install open`; o pendrive serve para o mesmo controller. |
| 409 / nome já usado | HTTP 409 do `/sign` do convite: este convite já assinou o certificado com esse nome (cada nome vale uma vez por convite). Abra outro convite (`sudo aic cluster invite`) ou use outro nome. Se o 409 vier do registro no controller, o nome já existe lá: revogue o antigo (`aic cluster node revoke`). |
| `aic worker change`: "worker já registrado com outra credencial" | O novo controller já tem um worker com esse id e outra credencial. No **novo** controller rode `aic cluster node revoke <id>` e repita `aic worker change`; o comando mostra isso em vez de esperar os 30 s. |

### Pendrive pelo navegador (Windows, Linux ou Mac)

Precisa de um controller instalado com uma release que traz a imagem x86 (`install.sh` mostra "modo dedicado (imagem x86 e pendrive)"). Sem ela, o bloco "Dedicar um computador" diz que o pendrive está indisponível nesta release.

No controller, abra o convite (ele mostra o endereço do pendrive):

```bash
sudo aic cluster invite        # ex.: http://192.168.1.10:8099/usb.zip
```

No computador novo (Windows, por exemplo), com um pendrive de **até 32 GB** (ele é apagado):

1. No navegador, abra o endereço do `usb.zip` e salve o arquivo. Confira o **SHA-256** dele com o que o convite mostra (no Windows: `certutil -hashfile usb.zip SHA256`). Se não bater, não use o arquivo: alguém na rede pode ter trocado o pendrive.
2. Formate o pendrive: Explorador de Arquivos > clique com o botão direito na unidade > **Formatar** > Sistema de arquivos **FAT32** > Iniciar (o rótulo tanto faz). (A janela do Windows não oferece FAT32 em pendrives acima de 32 GB.)
3. Dê dois cliques em `usb.zip`, selecione tudo (Ctrl+A) e copie para a **raiz** do pendrive (a unidade, não uma pasta). Devem aparecer as pastas `EFI` e `aic`, e o `LEIAME.txt`.
4. Na BIOS/UEFI do computador, **desligue o Secure Boot**. Desligue o computador, ligue o pendrive e abra o menu de boot (F12, F11, F9, F8 ou Esc, conforme a placa). Escolha a opção com **UEFI** no nome.
5. A tela mostra a impressão digital do aparelho e "aguardando aprovação". No controller, confira a impressão e aprove com o disco a apagar (o convite já mostra a linha pronta): `sudo aic cluster install approve <impressão> --disk <disco>`.
6. Depois da sua aprovação, o disco escolhido é apagado (contagem de 10 s), a imagem é gravada e a tela pede para tirar o pendrive. O computador reinicia pelo disco, já como worker do cluster.

O pendrive não é assinado: a proteção contra troca é o SHA-256 conferido fora da rede e a assinatura da imagem, que o instalador confere com a chave pinada antes de apagar qualquer disco. A pasta da imagem fica em `/opt/ai-cluster/dedicado`, do root, fora da pasta que o serviço grava.

**SSH no worker instalado pelo claim.** A imagem genérica não traz chave, então o `ClaimInstall` entrega as chaves públicas do operador, lidas do arquivo `AI_CLUSTER_AUTHORIZED_KEYS` (padrão do `setup`: `/etc/ai-cluster/authorized_keys`, `root:ai-cluster` 0640). O `setup` o preenche, na instalação do controller, na atualização e ao abrir o convite, com as chaves de `~/.ssh/authorized_keys` de quem invocou o `sudo` (ou de `/root`), e só se o arquivo ainda não existe ou está vazio: o que o operador já pôs nunca é sobrescrito. Só entram linhas `ssh-ed25519`, `ssh-rsa` ou `ecdsa-sha2-*` sem opções (`command=`, `from=`...), até 16. O `aic-wait --claim ... --ssh-out <arquivo>` grava as chaves (0600) e o `install-disk.sh --ssh <arquivo>` as põe em `/ssh/authorized_keys` da partição de dados (dir 0700, root). Entre como `root` (`ssh root@<ip-do-worker>`) e defina a senha do console com `aic worker passwd`. Sem chave no controller o worker instala igual, mas sem SSH, e o instalador avisa na tela final; para ter, ponha a `.pub` no arquivo e convide de novo.

**Certificado do túnel RPC no worker instalado pelo claim.** Sem ele o `ggml-rpc-server` não liga e o worker não entra na inferência distribuída. O instalador gera no aparelho a chave EC P-256 (`/run/rpc-tls/worker.key`, 0600, só em RAM) e um pedido de certificado; o `aic-wait --claim ... --csr <pedido> --rpc-out <pasta>` o manda junto do token, e o controller (se tem a CA, `AI_CLUSTER_RPC_CA_CERT`/`KEY`) devolve o certificado assinado (SAN `aic-worker-<id>`; o nome vem do id do worker, nunca do pedido) e o `ca.crt`. O `install-disk.sh --rpc-tls <pasta>` grava `worker.key` (0600), `worker.crt` e `ca.crt` (0644) em `/rpc-tls` da partição de dados (dir 0700, root), e o serviço do worker passa o dono ao usuário dele no boot; o worker lê de `/data/rpc-tls`. A chave nunca sai do aparelho. Só com a porta de espera em TLS (no claim em claro nada é assinado, para ninguém trocar a CA). Controller sem CA (laboratório sem PKI): o claim segue sem certificado e a tela final avisa que o worker ficou fora da inferência distribuída. O certificado vale 825 dias e a renovação (ADR-026) fixa a chave na primeira vez.

O `setup` grava no `controller.env` `AI_CLUSTER_PUBLIC_URL=http://<IP>:50051` (o endereço que o aparelho usa) e, como o controller instalado por ele é sem TLS, `AI_CLUSTER_ALLOW_PLAIN_CLAIM=1` (também acrescentados na atualização, se faltarem; controller com TLS recebe `https://` e sem o claim em claro). Impacto: sem TLS o claim entrega o join token em claro na rede; use só em rede fechada. O `aic-wait` grava `AI_CLUSTER_INSECURE=1` no `worker.env` quando o controller entregue é `http://`. Ao fim da gravação o instalador mostra "RETIRE O PENDRIVE" e espera Enter (sem teclado, reinicia em 60 s); `--efi-entry` põe a entrada "AI Cluster OS" em primeiro na BootOrder.

O pendrive guarda o endereço do controller na linha do kernel (`aic/grub.cfg`), então ele serve para aquele convite. O pendrive só precisa ser FAT32 com o `usb.zip` extraído na raiz: o GRUB percorre todos os discos e partições (MBR ou GPT, em qualquer ordem) até achar `/aic/grub.cfg`, sem depender de rótulo ou letra de unidade, e mostra um menu: **Instalar AI Cluster OS** (entra sozinho em 5 s; setas + Enter para navegar) e **Reiniciar o computador**. Não é preciso digitar nada no GRUB. Se não achar o zip, mostra a mensagem "extraia o usb.zip na RAIZ do pendrive" e um menu com Tentar de novo, Reiniciar e Desligar. Sem o zip extraído na raiz não dá boot no instalador: o `LEIAME.txt` do zip traz o mesmo passo a passo.

## Em um computador só, com Docker

O caminho mais curto (`deploy/`): `bash deploy/init.sh` gera um token por papel (controller, gateway e worker recebem só o que precisam) e a PKI do túnel RPC (uma CA, o certificado do gateway e o dos workers, 30 dias); `docker compose -f deploy/docker-compose.yml up -d --build --scale worker=2` constrói uma imagem única (controller, gateway, worker, CLI, `llama-server` e `rpc-server` do llama.cpp com o patch) e sobe tudo. Os modelos GGUF ficam em `deploy/models`. O passo a passo está no [README](../../README.md#início-rápido-em-um-computador-docker) e o fluxo dos modelos em [[modelos]]. As portas do host mudam com `AI_CLUSTER_GRPC_PORT` e `AI_CLUSTER_HTTP_PORT`.

O compose usa `AI_CLUSTER_INSECURE=1` na rede privada dele; fora dela configure o TLS do gRPC (seção abaixo). O túnel RPC é sempre mTLS.

## Compilar

A partir de `code/` (o workspace Cargo fica nessa pasta):

```bash
cargo build --release --workspace
```

Os binários saem em `code/target/release/`:

| Binário | Função |
|---|---|
| `ai-cluster-controller` | Controller |
| `ai-cluster-worker` | Worker |
| `ai-cluster-gateway` | Gateway de inferência |
| `aic` | CLI (o comando único; ver [[cli]]) |

## Cluster de desenvolvimento (só para quem desenvolve o projeto)

`scripts/dev-cluster.sh` (ou `scripts/dev-cluster.ps1` no Windows) sobe um cluster em contêineres Docker, na mesma máquina:

```bash
bash code/scripts/dev-cluster.sh build      # compila no volume aic-target
bash code/scripts/dev-cluster.sh up 2       # controller + 2 workers
bash code/scripts/dev-cluster.sh cli nodes  # roda `aic cluster nodes` contra ele
bash code/scripts/dev-cluster.sh metrics    # /metrics do controller
bash code/scripts/dev-cluster.sh down
```

O script usa tokens fixos de teste, `AI_CLUSTER_INSECURE=1` (sem TLS) e `AI_CLUSTER_AUTO_APPROVE=1` (workers entram aprovados). Não use essa configuração em rede aberta. A imagem base vem de `AIC_RUST_IMAGE` (padrão `rust:1-bookworm`).

## Produção com TLS

1. Gere a CA e o certificado do controller em PEM (o repositório não emite certificados). O certificado precisa cobrir o nome ou IP que os clientes usam.
2. Suba o controller com tokens distintos e o par de certificado:

   ```bash
   AI_CLUSTER_JOIN_TOKEN=<token-de-join> \
   AI_CLUSTER_OPERATOR_TOKEN=<token-de-operador> \
   AI_CLUSTER_TLS_CERT=/etc/aic/server.pem \
   AI_CLUSTER_TLS_KEY=/etc/aic/server.key \
   AI_CLUSTER_DATA_DIR=/var/lib/aic \
   ai-cluster-controller
   ```

   Sem o par de certificado o controller não sobe (a menos de `AI_CLUSTER_INSECURE=1`). `AI_CLUSTER_DATA_DIR` deve estar em volume que sobrevive a reinício: nele ficam o registro de workers (`nodes`), a fila (`state.bin`) e os eventos (`events.log`).
3. Suba cada worker apontando para `https://` e a CA:

   ```bash
   AI_CLUSTER_CONTROLLER=https://controller.exemplo:50051 \
   AI_CLUSTER_TLS_CA=/etc/aic/ca.pem \
   AI_CLUSTER_JOIN_TOKEN=<token-de-join> \
   AI_CLUSTER_NODE_ID=no-1 \
   AI_CLUSTER_NODE_CREDENTIAL_FILE=/var/lib/aic/credential \
   ai-cluster-worker
   ```

   Se o endpoint for um IP, defina `AI_CLUSTER_TLS_DOMAIN` com o nome do certificado.
4. Aprove cada worker novo: `aic cluster node approve no-1 --fingerprint <fp>`. Sem `AI_CLUSTER_AUTO_APPROVE`, o worker fica pendente e não recebe trabalho.
5. Suba a CLI com `AI_CLUSTER_CONTROLLER`, `AI_CLUSTER_OPERATOR_TOKEN` e `AI_CLUSTER_TLS_CA`.
6. Suba o gateway (ver [[api]]) com `AI_CLUSTER_API_KEY` ou `AI_CLUSTER_TENANTS`, pelo menos um dos tokens de operador ou de inferência, e o controller em `https://`.

Para mTLS (certificado de cliente), veja [[operacao]].

## Worker em Linux existente

`os/bootstrap.sh` põe o worker num Linux que já está instalado, com o endereço do controller e o token de join. Execute como root:

```bash
sudo ./os/bootstrap.sh --controller https://controller.exemplo:50051 \
  --token-file /root/join-token \
  [--binary ./ai-cluster-worker] [--node-id no-1] [--prefix /opt/ai-cluster] \
  [--model-dir /var/lib/ai-cluster/models] [--insecure] [--force] [--health-timeout 90]
```

- O token vai por arquivo (`--token-file`) para não aparecer na linha de comando. `--token` existe, mas expõe o segredo.
- Cria o usuário `ai-cluster`, instala o binário em `<prefix>/bin` (dono root) e grava `<prefix>/worker.env` (`root:ai-cluster`, 0640).
- Com systemd, instala uma unit com endurecimento (`NoNewPrivileges`, `ProtectSystem=strict`, escrita só em `state/`). Sem systemd, sobe o worker em segundo plano.
- `http://` exige `--insecure`. Valores são validados contra uma lista de caracteres permitidos. `--prefix` e `--model-dir` precisam ser absolutos.
- Trocar `--node-id` numa máquina já instalada exige `--force`.
- Rodar de novo atualiza binário e ambiente. Com systemd, a nova versão só fica se o worker se registrar em `--health-timeout` segundos; senão o binário anterior (`bin/ai-cluster-worker.prev`) volta e o script sai com código 3.
- O script não aprova o worker. Aprove com `aic cluster node approve <id> --fingerprint <fp>` quando for o caso.

## Contêiner do worker

```bash
cd code
docker build -f os/Dockerfile.worker -t ai-cluster-worker:dev .
```

A imagem tem o binário e um usuário sem privilégios (UID 10001), sobre `debian:bookworm-slim`. Nenhuma configuração entra na imagem: controller, token de join e id do worker vêm por variável. A credencial do worker fica em `AI_CLUSTER_NODE_CREDENTIAL_FILE` (`/var/lib/ai-cluster/credential` na imagem); monte um volume ali para o worker manter a aprovação depois de reiniciar.

## RPC do llama.cpp no worker (modelos GGUF)

Para o worker dividir um modelo GGUF com o gateway, ele roda o `ggml-rpc-server` do llama.cpp atrás de um túnel mTLS (veja [[seguranca]]). Gere o certificado do worker (SAN `aic-worker`, ou o nome combinado com o gateway) e a CA que assina **só o gateway** (a renovação automática depende de `AI_CLUSTER_RPC_CA_CERT` e `_KEY` no controller, ADR-026), e configure `AI_CLUSTER_RPC_SERVER_BIN`, `AI_CLUSTER_RPC_TLS_CERT`, `AI_CLUSTER_RPC_TLS_KEY` e `AI_CLUSTER_RPC_TLS_CLIENT_CA` ([[configuracao]]). Sem os três do TLS o worker segue sem RPC.

Num **Linux x86_64 com systemd**, `bash code/scripts/package-worker.sh` (depois de `docker compose -f deploy/docker-compose.yml build`) gera `dist/ai-cluster-worker-x86_64.tar.gz` com o worker, o `ggml-rpc-server`, o `bootstrap.sh` e um `install.sh`: copie, extraia e rode `sudo ./install.sh --controller ... --token-file ... --rpc-tls-dir ...` (precisa de `libgomp1` e glibc 2.36 ou mais nova). Na **imagem armv7** o `ggml-rpc-server` já vem em `/usr/bin` e liga sozinho quando existem `/data/rpc-tls/worker.crt`, `worker.key` e `ca.crt`. O binário é o do patch de 32 bits de `code/third_party/llama.cpp` (compilado por `build.sh`, no volume `aic-llama`, e copiado por `build-rootfs.sh`). A hora do worker precisa estar certa para o certificado valer: a imagem traz o DNS de reserva (`aic-dns`, o gateway da rota e 1.1.1.1 quando o DHCP não entrega nomes) e o chrony com salto de hora; uma placa sem RTC nasce em 2024 e acerta o relógio depois da rede. Se o worker não entra no plano com `certificate expired`, confira `date` nele.

## Imagem do AI Cluster OS (armv7)

Hoje existe imagem de disco só para placas armv7 com u-boot. A imagem é Alpine 3.20 com OpenRC, worker em musl, raiz ext4 somente leitura, dois slots de sistema (A e B, 512 MiB cada) e uma partição de dados. Não há imagem para ARM64 nem x86.

Sequência de build, a partir de `code/`:

1. Gere o par de chaves de release (uma vez; a privada fica em `~/.aic` e não sai da sua máquina):
   `bash os/image/release-keys.sh`
2. Compile o worker em musl, obrigatório para a imagem Alpine:
   `TARGET=armv7-unknown-linux-musleabihf bash scripts/build-armv7.sh`
3. Monte o rootfs (imagem ext4 do slot):
   `bash os/image/build-rootfs.sh [512M]`
4. Monte o disco com o layout A/B. Os três argumentos são o cabeçalho do u-boot (o primeiro MiB de um cartão que já dá boot na placa), o `zImage` e o `dtb` da placa:
   `bash os/image/mk-disk-image.sh <uboot-header.bin> <zImage> <dtb>`
   O resultado é `os/image/build/disk.img.gz`.
5. Grave o cartão. A gravação é feita a partir de um ambiente de recuperação em RAM, com `os/image/install-disk.py`:

   ```bash
   AIC_SSH_PASSWORD=... python3 os/image/install-disk.py --host <ip-do-no> --yes \
     --image os/image/build/disk.img.gz --controller https://controller.exemplo:50051 \
     --token <token> --node-id no-1 --ssh-key ~/.ssh/id.pub
   ```

   O instalador só age num worker com o cartão desmontado e rodando do initramfs (raiz `rootfs`). Ele apaga o cartão inteiro e exige `--yes`. Precisa do `paramiko`.

Para o worker chegar ao ambiente de recuperação pela rede, `os/netboot/` traz `build-initramfs.sh` (monta o initramfs com o worker e um servidor SSH), `tftpd.py` (servidor TFTP somente leitura; `--allow` lista os IPs que podem baixar) e `install-netboot.sh` (liga o boot pela rede num Linux armv7 existente, com prazo e volta ao cartão).

Releases assinadas para a atualização pelo controller são geradas com `os/image/sign-release.sh` (AI Cluster OS) e com `scripts/package-release.sh` (Linux, `SHA256SUMS.sig`). Veja [[operacao]].

## Imagem com a sua própria chave (fora da release)

A imagem do AI Cluster OS só instala atualização assinada por uma chave cuja **pública está dentro da própria imagem** ([[adr-018-release-assinada-e-medida-do-slot]]). Por isso uma imagem da release do projeto não aceita atualizações que você assinar, e vice-versa: para atualizar pelo seu controller, gere a imagem com a sua chave.

1. **Gere o par Ed25519** (uma vez). A privada fica em `~/.aic/release.key` (0600) e **nunca** vai para o repositório nem para a imagem; a pública, em `release.pub`:
   `bash os/image/release-keys.sh` (outro diretório: `AIC_KEYS=/cofre bash os/image/release-keys.sh`).
2. **Monte a imagem com a sua pública.** `os/image/build-rootfs.sh` (armv7) e `os/image/x86/build-rootfs.sh` copiam as públicas de `AIC_PUBS` (padrão `~/.aic/release.pub`) para `/etc/aic-release.d/` da imagem. Depois siga os passos das seções acima (`mk-disk-image.sh` ou `mk-disk.sh`, e a gravação).
3. **Assine cada versão nova e publique:**
   `bash os/image/sign-release.sh <versão> <rootfs.gz> <rootfs.ext4> [placa] [chave]` imprime os argumentos de `aic cluster rollout`. A placa é `armv7-opipc` (padrão) ou `x86_64-uefi`; a versão só cresce (números e pontos). Hospede o `rootfs.gz` num endereço que os workers alcancem e rode `aic cluster rollout <versão> <argumentos impressos> --url <endereço>` ([[operacao]]).
4. **Troque a chave sem perder os workers:** `bash os/image/release-keys.sh nova` gera `nova.key` e `nova.pub`. Monte uma imagem com `AIC_PUBS="~/.aic/release.pub ~/.aic/nova.pub"` (qualquer uma das duas confere), publique-a assinada com a antiga e, na imagem seguinte, só com a `nova.pub`, a antiga deixa de valer. A versão só cresce, então uma release antiga não volta.

Perder a chave privada significa não poder mais atualizar os workers por rollout: eles só voltam a aceitar atualização regravando a imagem com uma pública nova. Guarde a privada fora da máquina de build quando puder.

## Outras arquiteturas

Para um Linux ARM64 qualquer, compile o binário para essa arquitetura e use `bootstrap.sh` ou o contêiner. Não há imagem de disco nem atualização A/B para ARM64.

## Imagem do AI Cluster OS (x86_64 UEFI)

A imagem x86_64 formata o disco do PC, notebook ou servidor e o deixa só como worker: GPT com ESP, slots A e B e `/data`, GRUB EFI, kernel `linux-lts` do Alpine dentro do slot, atualização assinada e rollback automático ([[adr-019-imagem-x86-uefi-com-grub]]). **Validada em QEMU/UEFI e num notebook real (veja "Hardware e requisitos").** Requer UEFI com Secure Boot desligado. Os comandos rodam em `code/`, numa máquina com Docker e as chaves de release (`release-keys.sh`):

```bash
bash scripts/build-armv7.sh            # só na primeira vez: cria a imagem de build aic-rust-zig:1
bash scripts/build-x86.sh              # worker musl x86_64
bash os/image/x86/build-rootfs.sh                       # versão = a do produto (scripts/version.sh); build/rootfs-x86.ext4 (kernel, initramfs e worker)
bash os/image/x86/mk-disk.sh --controller https://controller.exemplo:50051 \
  --token <token> --node-id no-1 [--insecure] [--ssh-key chave.pub]   # build/disk-x86.img
```

Sem `--controller`, `--token` e `--node-id` (ou com `--insecure` sozinho, o que é recusado) a imagem é **genérica**: sai sem `/worker.env`, e a identidade entra depois, na gravação. Com as três opções, a imagem leva a identidade do worker, então é uma por worker. Para gravar num disco, a partir de qualquer Linux (um pendrive de recuperação serve), com o disco de destino desmontado:

```bash
sudo sh os/image/x86/install-disk.sh build/disk-x86.img /dev/sdX --yes   # APAGA o disco inteiro
sudo sh os/image/x86/install-disk.sh build/disk-x86.img /dev/sdX --yes   --env worker.env --efi-entry   # opcionais: grava worker.env no /data e cria a entrada de boot
```

`--env <arquivo>` grava o arquivo como `/worker.env` (modo 0600, dono root) na partição de dados, com `debugfs`, sem montar; precisa de `debugfs` e `cmp` (e2fsprogs). `--efi-entry` cria a entrada "AI Cluster OS" com `efibootmgr` na NVRAM **da máquina que roda o script**: serve quando ela é o próprio PC; num pendrive de recuperação, a entrada não vai para o PC de destino. Sem `efibootmgr` só avisa. O caminho `EFI/BOOT/BOOTX64.EFI` já dá boot sem entrada.

O script grava a imagem, move a GPT de backup para o fim, sorteia os GUIDs e estende a partição de dados até o fim do disco. Depois, dê boot pelo disco em modo UEFI. As releases para atualizar o worker são assinadas com a placa `x86_64-uefi`: `bash os/image/sign-release.sh <versão> <rootfs.gz> <rootfs.ext4> x86_64-uefi` (o `.gz` e o `.ext4` são o `build/rootfs-x86.ext4` comprimido e sem comprimir).

Para experimentar sem hardware, `bash os/image/x86/run-qemu.sh` sobe o disco num QEMU com OVMF dentro de um contêiner na rede `aic-net` (o boot sem KVM leva alguns minutos); `docker logs aic-vm` mostra o console. Sem firmware de hardware (`linux-firmware`) a imagem só enxerga placas de rede e de disco que funcionam sem blobs; para as demais, acrescente o pacote de firmware correspondente ao `x86/Dockerfile`.

### Ambiente de instalação (aprovação pelo controller)

`bash os/image/x86/build-installer.sh [chave-pública]` (padrão: `os/linux.pub`, a chave de Linux que assina o `SHA256SUMS` do dedicado; não a do rollout do SO) monta `build/installer/vmlinuz` e `build/installer/initramfs.gz` ([[adr-023-instalacao-com-aprovacao-do-controller]]): um ambiente que roda em RAM, com o `linux-lts`, busybox, `aic-wait`, `install-disk.sh` e a chave pública que assina o `dedicado` pinada (`/etc/aic/release.pub`; o padrão é `os/linux.pub`, a mesma de `package-release.sh`: se a pinada for outra, o instalador recusa a imagem com "assinatura do SHA256SUMS não confere"). Ao dar boot, ele sobe a rede por DHCP (mostra o IP e o MAC de cada placa), se há mais de um disco que pode ser apagado (internos; removíveis só se forem a única opção), mostra na tela do próprio computador uma lista com tamanho, modelo e tipo, com o maior interno sugerido, e deixa escolher com as setas e Enter (ou o número do disco), com o aviso de que o disco escolhido será APAGADO (inclusive o Windows); com um só candidato apenas informa, e sem tecla em 60 s segue com o sugerido. A escolha é só o disco anunciado como preferido: `aic cluster install approve` o sugere, e nada é apagado antes da aprovação com confirmação. Lê o controller de `aic.controller=` na linha do kernel ou de `/aic/installer.conf`, mostra a impressão digital e espera a aprovação (`aic cluster install approve <impressão> --disk <disco>`). Aprovado, baixa `disk-x86.img.gz` e o `SHA256SUMS` assinado e confere a assinatura e o SHA-256 **antes de qualquer escrita**. Confere o disco aprovado com o anunciado (nome, tamanho e modelo), deixa de fora o pendrive de instalação (proteção extra: partição com rótulo AIC, que o pendrive não precisa ter) e os discos removíveis, que só são apagados com `--force-removable`, mostra 10 s para desligar, pega o token de uso único e grava com `install-disk.sh --env --efi-entry`. A imagem comprimida fica na RAM, então a máquina precisa de RAM para ela. Em qualquer falha, o ambiente diz o que fazer e para; não reinicia sozinho.

A mídia de boot é o `usb.zip` do convite (seção acima): `build-installer.sh` gera também `build/installer/BOOTX64.EFI` (GRUB com o cfg embutido `x86/embed.cfg`, que percorre os discos atrás de `/aic/grub.cfg`; o GRUB não aceita bloco `{ }` solto depois de `||`, dá `syntax error` e cai no prompt `grub>`). O controller serve `/imagem/disk-x86.img.gz`, `SHA256SUMS`, `SHA256SUMS.sig` e `VERSION` enquanto o convite está aberto. Para isso, a release precisa de `dedicado.tar.gz`: `AIC_RELEASE=<versão> bash os/image/x86/build-installer.sh`, `os/image/x86/mk-disk.sh` (com `build/disk-x86.img.gz` ou `.img`) e depois `bash code/scripts/package-release.sh`, que assina a pasta com a chave de Linux. Controllers já instalados só ganham o modo dedicado com uma instalação nova (ver [[pendencias]]).

Testado sem hardware: `code/tests/phase-u-usb.sh` (estrutura do zip, chave de teste, servidor do convite e conferência do `aic-install.sh`). O boot do pendrive foi reproduzido e corrigido em QEMU/OVMF (pendrive MBR e GPT, qualquer ordem de discos, até o instalador subir). Num PC real, o boot do pendrive com o cfg atual ainda precisa ser revisto em mais modelos.

## Pesos dos modelos

A inferência usa pesos locais que o repositório não versiona (`code/models/` é ignorado pelo git). Baixe do Hugging Face para estas pastas:

| Pasta em `code/models/` | Origem | Arquivos |
|---|---|---|
| `gpt2/` | `openai-community/gpt2` | `model.safetensors`, `vocab.json`, `merges.txt`, `config.json` |
| `smollm2/` | `HuggingFaceTB/SmolLM2-135M` | `model.safetensors`, `tokenizer.json`, `merges.txt`, `config.json`, `generation_config.json` |
| `smollm2-instruct/` | `HuggingFaceTB/SmolLM2-135M-Instruct` | mesmos arquivos do SmolLM2 |

Exemplo: `huggingface-cli download openai-community/gpt2 model.safetensors vocab.json merges.txt config.json --local-dir code/models/gpt2`.

O diretório é apontado por `AI_CLUSTER_GPT2_DIR` e `AI_CLUSTER_SMOLLM2_DIR` (veja [[configuracao]]). Sem os pesos, o cluster, a CLI e o benchmark funcionam; só a inferência de linguagem fica indisponível.

## Serviços no boot

Depois de `aic controller install`, `aic gateway install` ou `aic worker join` (e do instalador), os serviços ficam habilitados e sobem sozinhos ao ligar o computador:

- **Linux com systemd:** o instalador grava as units `ai-cluster-controller`, `ai-cluster-gateway` e `ai-cluster-worker` em `/etc/systemd/system/`, com `systemctl enable` (`WantedBy=multi-user.target`) e `Restart=always`, para voltar depois de um reinício ou de uma queda. A configuração fica em `/etc/ai-cluster/*.env`, e os binários em `/opt/ai-cluster/bin`.
- **AI Cluster OS (OpenRC):** o worker, o `aic-updater`, o `aic-slot-commit` e o `aic-dns` entram no runlevel `default` da imagem; a sequência de boot está em `os/image/Dockerfile`.
- **Contêiner sem systemd:** não há como habilitar serviço. Os processos sobem em segundo plano e **não voltam sozinhos** depois de reiniciar o contêiner. O instalador avisa disso; depois de um reinício, rode o `aic` de novo para subir os serviços.

Para conferir: `aic controller status`, `aic gateway status` ou `aic worker status`. Os serviços sobem com as configurações da instalação; mudanças de configuração estão em [[configuracao]].

## Cabeçalho e cores

Toda saída pensada para uma pessoa (o instalador e o `aic`) começa com o logotipo da malha de sete workers e a linha `AI.Cluster <componente> v<versão>`, com as mesmas cores em todos os componentes. Sem terminal (log, pipe, arquivo) ou com `NO_COLOR`, o cabeçalho vira uma linha só, sem cor. `AI_CLUSTER_COLOR=never` tira só a cor; `AI_CLUSTER_COLOR=always` força a cor; `AI_CLUSTER_ASCII=1` troca os glifos por ASCII.

## Comandos antigos

Os nomes da versão anterior deixaram de existir, sem apelidos. A tabela de troca está em [[cli#Migração dos comandos]].
