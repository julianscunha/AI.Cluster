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
curl -fsSL https://github.com/julianscunha/Real.AI.Cluster/releases/latest/download/install.sh | sudo sh
```

Em inglês: acrescente `-s -- --lang en` ao `sh` (ou `AIC_LANG=en`). O idioma vale para o instalador, o `aic-setup` e os scripts do convite, e fica gravado em `/etc/ai-cluster/lang`. Sem isso tudo sai em português. Em modo inglês, `--mode dedicated`, `--kind physical` e `--kind container` também são aceitos.

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
sudo aic-setup invite
# 3. em cada outro computador, o mesmo instalador, escolhendo "Worker" (ou a linha curta do convite)
curl -fsSL http://<ip-do-servidor>:8099/join | sudo sh
```

O pacote do worker de cada processador vem do próprio servidor, na mesma versão do cluster: o servidor o baixa da release na primeira vez que abre um convite e o guarda em cache. Os outros computadores não precisam de internet. Cada worker gera a própria chave do túnel RPC; o servidor só assina o pedido de certificado (SAN `aic-worker-<nome do nó>`), e a chave nunca sai do worker. O gateway em outro computador entra pelo mesmo convite (`sudo aic-setup gateway --server <ip> --code <código>`), mas a pasta de modelos precisa ser a mesma que o controller enxerga (NFS, por exemplo).

Sem perguntas (automação): `sudo sh install.sh --mode linux --role server|controller|gateway|worker --arch x86_64|armv7 --kind fisico|vm|conteiner [--server IP] --yes`.

O `install.sh` baixa `VERSION`, `SHA256SUMS` e os pacotes `tools`, `controller-linux-<arq>`, `gateway-linux-x86_64` e `worker-linux-<arq>`, e confere o SHA-256 de cada um. `bash code/scripts/package-release.sh` gera esses arquivos (nomes sem versão, para `releases/latest/download/`). Sobre um Linux existente o desempenho é compartilhado com o que já roda nele e o cluster confia no sistema operacional da máquina; o modo dedicado existe para evitar os dois problemas.

### Apresentar um computador para instalar o AI Cluster OS

No convite aberto (`sudo aic-setup invite`) o servidor mostra também a linha para quem já tem Linux e quer virar um worker dedicado:

```bash
curl -fsSL http://<ip-do-servidor>:8099/wait | sudo sh
```

O script confere o processador, o firmware e o espaço, baixa só o `aic-wait` do servidor (SHA-256 conferido) e o roda. A tela mostra a **impressão digital**, o MAC, a CPU, a RAM e os discos. No servidor: `ai-cluster installs` lista os aparelhos; confira a impressão digital com a da tela e aprove com `ai-cluster approve-install <impressão> --disk <disco>` (a aprovação apaga o disco informado). `ai-cluster reject-install` tira da lista. A porta de espera só aceita aparelhos enquanto o convite está aberto (ou `ai-cluster enrollment open`). O reinício para a imagem de espera e a instalação em si ainda não existem ([ADR-023](../system/05-decisoes/adr-023-instalacao-com-aprovacao-do-controller.md)).

## Instalação dedicada

Dedicar o computador grava o AI Cluster OS no disco: um sistema só para o cluster, imutável, com slots A/B, volta automática e release assinada ([[adr-017-instalacao-a-b-e-boot-de-rede]], [[adr-019-imagem-x86-uefi-com-grub]]). É o equivalente ao ESXi: desempenho previsível, sem outros serviços competindo.

**Hoje:** só o **worker** tem imagem (armv7 e x86_64 UEFI). Controller e gateway dedicados ainda não estão nas imagens ([[pendencias]]). O instalador guiado já oferece a opção e explica isso.

**Ordem de deploy:** o controller vem primeiro. O worker só confirma o slot novo depois de se registrar no controller, e a imagem já leva o endereço dele e o join token.

**Como a imagem chega ao computador (previsto para o instalador guiado):**
- **Pela rede (boot de rede):** restrito. O usuário informa o **MAC** de cada worker e só esses endereços recebem a imagem; o boot de rede é só instalador e recuperação, nunca modo de operação. Hoje o servidor TFTP já limita as origens por IP (`--allow`), com limite de boots; falta o filtro por MAC.
- **Por pendrive:** quando a rede não tem DHCP ou o computador não é visível nela, o instalador gera o pendrive de instalação, guiado por perguntas.

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
| `ai-cluster` | CLI |

## Cluster de desenvolvimento (só para quem desenvolve o projeto)

`scripts/dev-cluster.sh` (ou `scripts/dev-cluster.ps1` no Windows) sobe um cluster em contêineres Docker, na mesma máquina:

```bash
bash code/scripts/dev-cluster.sh build      # compila no volume aic-target
bash code/scripts/dev-cluster.sh up 2       # controller + 2 workers
bash code/scripts/dev-cluster.sh cli nodes  # roda `ai-cluster nodes` contra ele
bash code/scripts/dev-cluster.sh metrics    # /metrics do controller
bash code/scripts/dev-cluster.sh down
```

O script usa tokens fixos de teste, `AI_CLUSTER_INSECURE=1` (sem TLS) e `AI_CLUSTER_AUTO_APPROVE=1` (nós entram aprovados). Não use essa configuração em rede aberta. A imagem base vem de `AIC_RUST_IMAGE` (padrão `rust:1-bookworm`).

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

   Sem o par de certificado o controller não sobe (a menos de `AI_CLUSTER_INSECURE=1`). `AI_CLUSTER_DATA_DIR` deve estar em volume que sobrevive a reinício: nele ficam o registro de nós (`nodes`), a fila (`state.bin`) e os eventos (`events.log`).
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
4. Aprove cada nó novo: `ai-cluster approve no-1 --fingerprint <fp>`. Sem `AI_CLUSTER_AUTO_APPROVE`, o nó fica pendente e não recebe trabalho.
5. Suba a CLI com `AI_CLUSTER_CONTROLLER`, `AI_CLUSTER_OPERATOR_TOKEN` e `AI_CLUSTER_TLS_CA`.
6. Suba o gateway (ver [[api]]) com `AI_CLUSTER_API_KEY` ou `AI_CLUSTER_TENANTS`, pelo menos um dos tokens de operador ou de inferência, e o controller em `https://`.

Para mTLS (certificado de cliente), veja [[operacao]].

## Worker em Linux existente

`os/bootstrap.sh` põe o worker num Linux que já está instalado. Execute como root:

```bash
sudo ./os/bootstrap.sh --controller https://controller.exemplo:50051 \
  --join-token-file /root/join-token \
  [--binary ./ai-cluster-worker] [--node-id no-1] [--prefix /opt/ai-cluster] \
  [--model-dir /var/lib/ai-cluster/models] [--insecure] [--force] [--health-timeout 90]
```

- O token vai por arquivo (`--join-token-file`) para não aparecer na linha de comando. `--join-token` existe, mas expõe o segredo.
- Cria o usuário `ai-cluster`, instala o binário em `<prefix>/bin` (dono root) e grava `<prefix>/worker.env` (`root:ai-cluster`, 0640).
- Com systemd, instala uma unit com endurecimento (`NoNewPrivileges`, `ProtectSystem=strict`, escrita só em `state/`). Sem systemd, sobe o worker em segundo plano.
- `http://` exige `--insecure`. Valores são validados contra uma lista de caracteres permitidos. `--prefix` e `--model-dir` precisam ser absolutos.
- Trocar `--node-id` numa máquina já instalada exige `--force`.
- Rodar de novo atualiza binário e ambiente. Com systemd, a nova versão só fica se o worker se registrar em `--health-timeout` segundos; senão o binário anterior (`bin/ai-cluster-worker.prev`) volta e o script sai com código 3.
- O script não aprova o nó. Aprove com `ai-cluster approve` quando for o caso.

## Contêiner do worker

```bash
cd code
docker build -f os/Dockerfile.worker -t ai-cluster-worker:dev .
```

A imagem tem o binário e um usuário sem privilégios (UID 10001), sobre `debian:bookworm-slim`. Nenhuma configuração entra na imagem: controller, token de join e id do nó vêm por variável. A credencial do nó fica em `AI_CLUSTER_NODE_CREDENTIAL_FILE` (`/var/lib/ai-cluster/credential` na imagem); monte um volume ali para o nó manter a aprovação depois de reiniciar.

## RPC do llama.cpp no worker (modelos GGUF)

Para o worker dividir um modelo GGUF com o gateway, ele roda o `ggml-rpc-server` do llama.cpp atrás de um túnel mTLS (veja [[seguranca]]). Gere o certificado do worker (SAN `aic-worker`, ou o nome combinado com o gateway) e a CA que assina **só o gateway**, e configure `AI_CLUSTER_RPC_SERVER_BIN`, `AI_CLUSTER_RPC_TLS_CERT`, `AI_CLUSTER_RPC_TLS_KEY` e `AI_CLUSTER_RPC_TLS_CLIENT_CA` ([[configuracao]]). Sem os três do TLS o nó segue sem RPC.

Num **Linux x86_64 com systemd**, `bash code/scripts/package-worker.sh` (depois de `docker compose -f deploy/docker-compose.yml build`) gera `dist/ai-cluster-worker-x86_64.tar.gz` com o worker, o `ggml-rpc-server`, o `bootstrap.sh` e um `install.sh`: copie, extraia e rode `sudo ./install.sh --controller ... --join-token-file ... --rpc-tls-dir ...` (precisa de `libgomp1` e glibc 2.36 ou mais nova). Na **imagem armv7** o `ggml-rpc-server` já vem em `/usr/bin` e liga sozinho quando existem `/data/rpc-tls/worker.crt`, `worker.key` e `ca.crt`. O binário é o do patch de 32 bits de `code/third_party/llama.cpp` (compilado por `build.sh`, no volume `aic-llama`, e copiado por `build-rootfs.sh`). A hora do nó precisa estar certa para o certificado valer: a imagem traz o DNS de reserva (`aic-dns`, o gateway da rota e 1.1.1.1 quando o DHCP não entrega nomes) e o chrony com salto de hora; uma placa sem RTC nasce em 2024 e acerta o relógio depois da rede. Se o nó não entra no plano com `certificate expired`, confira `date` nele.

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
     --join-token <token> --node-id no-1 --ssh-key ~/.ssh/id.pub
   ```

   O instalador só age num nó com o cartão desmontado e rodando do initramfs (raiz `rootfs`). Ele apaga o cartão inteiro e exige `--yes`. Precisa do `paramiko`.

Para o nó chegar ao ambiente de recuperação pela rede, `os/netboot/` traz `build-initramfs.sh` (monta o initramfs com o worker e um servidor SSH), `tftpd.py` (servidor TFTP somente leitura; `--allow` lista os IPs que podem baixar) e `install-netboot.sh` (liga o boot pela rede num Linux armv7 existente, com prazo e volta ao cartão).

Releases assinadas para a atualização pelo controller são geradas com `os/image/sign-release.sh`. Veja [[operacao]].

## Imagem com a sua própria chave (fora da release)

A imagem do AI Cluster OS só instala atualização assinada por uma chave cuja **pública está dentro da própria imagem** ([[adr-018-release-assinada-e-medida-do-slot]]). Por isso uma imagem da release do projeto não aceita atualizações que você assinar, e vice-versa: para atualizar pelo seu controller, gere a imagem com a sua chave.

1. **Gere o par Ed25519** (uma vez). A privada fica em `~/.aic/release.key` (0600) e **nunca** vai para o repositório nem para a imagem; a pública, em `release.pub`:
   `bash os/image/release-keys.sh` (outro diretório: `AIC_KEYS=/cofre bash os/image/release-keys.sh`).
2. **Monte a imagem com a sua pública.** `os/image/build-rootfs.sh` (armv7) e `os/image/x86/build-rootfs.sh` copiam as públicas de `AIC_PUBS` (padrão `~/.aic/release.pub`) para `/etc/aic-release.d/` da imagem. Depois siga os passos das seções acima (`mk-disk-image.sh` ou `mk-disk.sh`, e a gravação).
3. **Assine cada versão nova e publique:**
   `bash os/image/sign-release.sh <versão> <rootfs.gz> <rootfs.ext4> [placa] [chave]` imprime os argumentos de `ai-cluster rollout`. A placa é `armv7-opipc` (padrão) ou `x86_64-uefi`; a versão só cresce (números e pontos). Hospede o `rootfs.gz` num endereço que os nós alcancem e rode `ai-cluster rollout <argumentos impressos> --url <endereço>` ([[operacao]]).
4. **Troque a chave sem perder os nós:** `bash os/image/release-keys.sh nova` gera `nova.key` e `nova.pub`. Monte uma imagem com `AIC_PUBS="~/.aic/release.pub ~/.aic/nova.pub"` (qualquer uma das duas confere), publique-a assinada com a antiga e, na imagem seguinte, só com a `nova.pub`, a antiga deixa de valer. A versão só cresce, então uma release antiga não volta.

Perder a chave privada significa não poder mais atualizar os nós por rollout: eles só voltam a aceitar atualização regravando a imagem com uma pública nova. Guarde a privada fora da máquina de build quando puder.

## Outras arquiteturas

Para um Linux ARM64 qualquer, compile o binário para essa arquitetura e use `bootstrap.sh` ou o contêiner. Não há imagem de disco nem atualização A/B para ARM64.

## Imagem do AI Cluster OS (x86_64 UEFI)

A imagem x86_64 formata o disco do PC, notebook ou servidor e o deixa só como worker: GPT com ESP, slots A e B e `/data`, GRUB EFI, kernel `linux-lts` do Alpine dentro do slot, atualização assinada e rollback automático ([[adr-019-imagem-x86-uefi-com-grub]]). **Validada em QEMU/UEFI; ainda não em hardware real.** Requer UEFI com Secure Boot desligado. Os comandos rodam em `code/`, numa máquina com Docker e as chaves de release (`release-keys.sh`):

```bash
bash scripts/build-armv7.sh            # só na primeira vez: cria a imagem de build aic-rust-zig:1
bash scripts/build-x86.sh              # worker musl x86_64
AIC_RELEASE=1.0 bash os/image/x86/build-rootfs.sh        # build/rootfs-x86.ext4 (kernel, initramfs e worker)
bash os/image/x86/mk-disk.sh --controller https://controller.exemplo:50051 \
  --join-token <token> --node-id no-1 [--insecure] [--ssh-key chave.pub]   # build/disk-x86.img
```

A imagem leva a identidade do nó (`worker.env`), então é uma por nó. Para gravar num disco, a partir de qualquer Linux (um pendrive de recuperação serve), com o disco de destino desmontado:

```bash
sudo sh os/image/x86/install-disk.sh build/disk-x86.img /dev/sdX --yes   # APAGA o disco inteiro
```

O script grava a imagem, move a GPT de backup para o fim, sorteia os GUIDs e estende a partição de dados até o fim do disco. Depois, dê boot pelo disco em modo UEFI. As releases para atualizar o nó são assinadas com a placa `x86_64-uefi`: `bash os/image/sign-release.sh <versão> <rootfs.gz> <rootfs.ext4> x86_64-uefi` (o `.gz` e o `.ext4` são o `build/rootfs-x86.ext4` comprimido e sem comprimir).

Para experimentar sem hardware, `bash os/image/x86/run-qemu.sh` sobe o disco num QEMU com OVMF dentro de um contêiner na rede `aic-net` (o boot sem KVM leva alguns minutos); `docker logs aic-vm` mostra o console. Sem firmware de hardware (`linux-firmware`) a imagem só enxerga placas de rede e de disco que funcionam sem blobs; para as demais, acrescente o pacote de firmware correspondente ao `x86/Dockerfile`.

## Pesos dos modelos

A inferência usa pesos locais que o repositório não versiona (`code/models/` é ignorado pelo git). Baixe do Hugging Face para estas pastas:

| Pasta em `code/models/` | Origem | Arquivos |
|---|---|---|
| `gpt2/` | `openai-community/gpt2` | `model.safetensors`, `vocab.json`, `merges.txt`, `config.json` |
| `smollm2/` | `HuggingFaceTB/SmolLM2-135M` | `model.safetensors`, `tokenizer.json`, `merges.txt`, `config.json`, `generation_config.json` |
| `smollm2-instruct/` | `HuggingFaceTB/SmolLM2-135M-Instruct` | mesmos arquivos do SmolLM2 |

Exemplo: `huggingface-cli download openai-community/gpt2 model.safetensors vocab.json merges.txt config.json --local-dir code/models/gpt2`.

O diretório é apontado por `AI_CLUSTER_GPT2_DIR` e `AI_CLUSTER_SMOLLM2_DIR` (veja [[configuracao]]). Sem os pesos, o cluster, a CLI e o benchmark funcionam; só a inferência de linguagem fica indisponível.

## Controller e gateway como serviço (systemd)

O repositório instala a unit do worker (`bootstrap.sh`), mas não a do controller nem a do gateway. Exemplo para o controller, em `/etc/systemd/system/ai-cluster-controller.service`:

```ini
[Unit]
Description=AI Cluster controller
After=network-online.target
Wants=network-online.target

[Service]
EnvironmentFile=/etc/ai-cluster/controller.env
ExecStart=/opt/ai-cluster/bin/ai-cluster-controller
StateDirectory=aic
Restart=on-failure
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true

[Install]
WantedBy=multi-user.target
```

`/etc/ai-cluster/controller.env` (modo 0600, dono root) guarda as variáveis `AI_CLUSTER_*` do passo "Produção com TLS", uma por linha, no formato `CHAVE=valor`. Com `StateDirectory=aic` o estado fica em `/var/lib/aic`. Depois: `systemctl daemon-reload && systemctl enable --now ai-cluster-controller`. O gateway segue o mesmo modelo, trocando o binário e as variáveis.
