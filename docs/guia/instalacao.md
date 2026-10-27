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

## Cluster de desenvolvimento

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
   AI_CLUSTER_NODES=/var/lib/aic/nodes \
   AI_CLUSTER_STATE=/var/lib/aic/state.bin \
   AI_CLUSTER_EVENTS=/var/lib/aic/events.log \
   ai-cluster-controller
   ```

   Sem o par de certificado o controller não sobe (a menos de `AI_CLUSTER_INSECURE=1`). Os caminhos de `NODES`, `STATE` e `EVENTS` devem estar em volume que sobrevive a reinício.
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
4. Aprove cada nó novo: `ai-cluster approve no-1`. Sem `AI_CLUSTER_AUTO_APPROVE`, o nó fica pendente e não recebe trabalho.
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

## Outras arquiteturas

Para um Linux x86 ou ARM64 qualquer, compile o binário para essa arquitetura e use `bootstrap.sh` ou o contêiner. Não há imagem de disco nem atualização A/B para elas.

## Pesos dos modelos

A inferência usa pesos locais que o repositório não versiona (`code/models/` é ignorado pelo git). Baixe do Hugging Face para estas pastas:

| Pasta em `code/models/` | Origem | Arquivos |
|---|---|---|
| `gpt2/` | `openai-community/gpt2` | `model.safetensors`, `vocab.json`, `merges.txt`, `config.json` |
| `smollm2/` | `HuggingFaceTB/SmolLM2-135M` | `model.safetensors`, `tokenizer.json`, `merges.txt`, `config.json`, `generation_config.json` |
| `smollm2-instruct/` | `HuggingFaceTB/SmolLM2-135M-Instruct` | mesmos arquivos do SmolLM2 |

Exemplo: `huggingface-cli download openai-community/gpt2 model.safetensors vocab.json merges.txt config.json --local-dir code/models/gpt2`.

O diretório é apontado por `AI_CLUSTER_GPT2_DIR` e `AI_CLUSTER_SMOLLM2_DIR` (veja [[configuracao]]). Sem os pesos, o cluster, a CLI e o benchmark funcionam; só a inferência de linguagem fica indisponível.
