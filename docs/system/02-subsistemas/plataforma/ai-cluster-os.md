---
title: "AI Cluster OS"
aliases: ["AI Cluster OS", "Imagem do nó", "Sistema do nó"]
tags: [sistema/subsistema/plataforma]
type: sistema/subsistema
---

# AI Cluster OS

O AI Cluster OS é o sistema mínimo, imutável e orientado a compute de cada nó. Ele existe para que um nó seja só um worker: sem ambiente de desktop, sem serviços de consumo, com o runtime do worker instalado e supervisionado, e com atualização que se reverte. Não é uma distribuição nova. É uma imagem de uma distribuição Linux existente, montada e assinada pelo projeto. Voltar para [[plataforma]].

> Planejado: imagem para ARM64. As imagens armv7 (u-boot) e x86_64 (UEFI com GRUB, [[adr-019-imagem-x86-uefi-com-grub]]) existem; o x86_64 foi validado em QEMU, ainda não em hardware real.

## Imagem

As imagens são para armv7 (placas com u-boot) e x86_64 (PCs e servidores com UEFI), com a base Alpine Linux 3.20 e OpenRC. A parte comum (serviços, raiz somente leitura, release assinada, rollout) é a mesma; muda o boot e onde fica o estado do A/B. No x86_64 o kernel `linux-lts` está dentro do slot, o GRUB guarda o estado no `grubenv` da ESP e os slots são as partições 2 e 3 do disco da raiz.

- **Serviços:** só o worker, em binário musl, e o mínimo de apoio: servidor SSH somente com chave, sincronização de relógio e um gerador de entropia, porque a placa não tem fonte de aleatoriedade suficiente.
- **Raiz somente leitura.** `/var/log` e `/tmp` são tmpfs.
- **Partição `/data`** separada, para estado que sobrevive à troca de imagem: cache, fila local, credenciais e chaves SSH.
- **Watchdog de hardware** alimentado pelo sistema, para reiniciar a placa quando o sistema trava.

Caminhos relevantes para operação:

| Caminho | Conteúdo |
|---|---|
| `/etc/aic-release` | Versão instalada. |
| `/etc/aic-board` | Identificação da placa. A assinatura da release precisa ser dessa placa. |
| `/etc/aic-release.d/` | Chaves públicas de release. |
| `/run/aic-measure` | Medida do slot ativo, feita no boot. |
| `/data/state/` | Estado do worker, incluindo a marca de tentativa de atualização. |

Layout do cartão: uma área de cabeçalho do u-boot, uma partição FAT `AICBOOT` com o script de boot, os slots A e B de 512 MiB cada, e a partição `aicdata` que vira `/data`. O estado do boot (slot ativo, slot em teste, tentativas e contagem de boots) fica num setor cru, fora da FAT.

## Boot A/B

- Uma atualização grava o slot inativo. O boot seguinte usa esse slot em teste, com duas tentativas.
- O slot novo é confirmado depois que o worker se registra no controller. Se o controller não responde, a confirmação acontece mesmo assim.
- Se o worker não se registra em 300 s com o controller respondendo, o nó reinicia.
- Se o slot em teste falha nas duas tentativas, o u-boot volta ao slot anterior.
- Seis boots sem confirmação, ou um pedido explícito de recuperação, levam à recuperação por rede.

### Recuperação por rede

O u-boot obtém endereço por DHCP, baixa kernel, initramfs e árvore de dispositivos por TFTP e sobe um ambiente de recuperação em memória. Esse ambiente regrava os slots e devolve o nó ao cartão. O servidor TFTP é um script simples, com lista de permissão (`--allow`). Não tem TLS e não verifica assinatura do kernel.

## Instalação

Há três caminhos para colocar um nó no ar.

1. **Imagem em cartão.** `install-disk.py` roda a partir de um ambiente em memória e grava a imagem. Antes de gravar, confere a raiz do ambiente, que o cartão está desmontado e que a execução recebeu `--yes`. Depois grava a configuração do worker (`worker.env`, modo 0600), a chave SSH e o script de boot com as definições de recuperação.
2. **Boot pela rede.** `install-netboot.sh` faz a placa subir o ambiente de recuperação por TFTP, com retorno automático ao cartão.
3. **Bootstrap sobre Linux existente.** `os/bootstrap.sh`, executado como root, instala o worker num Linux que já existe.

```text
os/bootstrap.sh --controller URL --join-token-file ARQUIVO [--binary CAMINHO] [--node-id ID] [--prefix DIR] [--model-dir DIR] [--insecure]
```

- O token é lido de arquivo, para não aparecer na linha de comando.
- Todo valor é validado contra uma lista de caracteres permitidos antes de ser gravado, porque entra em arquivo de ambiente, em unidade de serviço e em comando.
- Caminhos são absolutos. Um prefixo que é link ou que não pertence ao root é recusado.
- Com systemd, cria uma unidade com isolamento: `NoNewPrivileges`, `ProtectSystem=strict`, escrita só em `state/`, sem capacidades. Sem systemd, sobe o processo em segundo plano, com log e pid em `state/`.
- Trocar o `--node-id` de uma máquina já instalada exige `--force`.
- Rodar de novo atualiza o binário e o ambiente, e troca o processo. O bootstrap não aprova o nó. A aprovação continua manual.

### Atualização do worker fora da imagem

Rodar o bootstrap de novo com outro `--binary` troca o binário. A versão anterior fica em `ai-cluster-worker.prev`. Com systemd, a nova versão só fica se o serviço ficar ativo e o worker se registrar no controller dentro de `--health-timeout` segundos (padrão 90). Caso contrário, a anterior volta e o script sai com código 3. Sem systemd, não há espera nem rollback.

## Atualização do sistema

Uma atualização troca a imagem inteira, não arquivos isolados. O operador assina a release, e o controller a distribui em rollout.

### Preparar a release

1. `sign-release.sh <versão> <rootfs.gz> <rootfs.ext4>` calcula os SHA-256 do arquivo comprimido e da imagem, assina e imprime os argumentos do rollout.
2. `ai-cluster rollout <versão> --url <url> --sha256 <hex> --image-sha256 <hex> --signature <base64>` define a release.
3. `ai-cluster nodes` mostra a versão e a medida de cada nó. `ai-cluster events` mostra o andamento.
4. `ai-cluster rollout --cancel` encerra o rollout.

Ver [[cli-tui]] para os argumentos e [[seguranca]] para a assinatura e a rotação de chaves.

### Como o rollout anda

- Só participam os nós que declaram capacidade de atualizar, isto é, têm o comando de atualização configurado. Os demais continuam na versão atual.
- O controller escolhe como **canário** o primeiro nó apto que informa a versão antiga. Os demais esperam o canário reportar a versão nova **e** a medida igual à da release assinada.
- `AI_CLUSTER_ROLLOUT_WAVE=N` limita quantos nós atualizam ao mesmo tempo, depois do canário. Ausente ou `0` significa todos de uma vez. Uma vaga volta quando o nó reporta a versão nova, ou depois de 10 min.
- Se o canário não reporta a versão nova em 10 min, o rollout pausa e registra um evento `rollout`. Ele só anda de novo com um novo comando `rollout`.
- Com `AI_CLUSTER_RELEASE_STATE`, a release desejada sobrevive ao reinício do controller.

### O que o nó faz

1. Recebe a release no heartbeat e a valida: caracteres, versão e formato.
2. Drena o trabalho, como no desligamento, e sai do cluster.
3. Roda o comando de atualização sem shell, que entrega o pedido ao processo de atualização do sistema.
4. O processo de atualização confere a assinatura, a versão e o espaço, baixa a imagem, grava o slot inativo, relê o slot e compara a medida com a assinada. Só então marca o slot em teste e reinicia.
5. Se o processo recusa o pedido, grava o motivo e o worker volta ao cluster em segundos. Caso contrário, espera até 20 min pelo reinício.

Uma marca guarda a versão que acabou de ser tentada. Se o nó volta por rollback, não tenta a mesma versão de novo.

## Identidade visual

As telas do sistema mostram a mesma marca (grafo de nós, "Real.AI.Cluster" e "AI Cluster OS"), só em ASCII para funcionar na serial e no console:

- **Login:** `/etc/issue` (antes do login, com nome do nó e tty) e `/etc/motd` (depois), no overlay comum.
- **Boot:** uma linha de marca no `boot.cmd` (U-Boot) e no `grub.cfg` (x86), sem o desenho, porque os dois interpretam `\`.
- **Console no monitor (tty1), no estilo do DCUI do ESXi:** o `aic-console` mostra uma tela de repouso parada, com o logotipo, a versão e o slot, um painel "Estado do nó" (papel, worker, rede, hardware, controller e registro, cada um com um ponto verde, amarelo ou vermelho) e a dica de como administrar o nó. Ele só repinta quando o conteúdo muda: nada pisca. `F2` abre o login e o menu "Personalizar o sistema" (registros do worker, rede em leitura, reiniciar o worker, reiniciar e desligar o nó, estado da atualização); `F12` abre reiniciar ou desligar. Não há shell nele: cada item chama um comando fixo.
- **Senha do console:** opcional. `aic passwd` (por SSH, root) grava `/data/console-pass` como hash SHA-512 com sal. Com senha, `F2` e `F12` pedem login (três erros travam a entrada por 30 s, dobrando a cada rodada); sem senha eles abrem direto e a tela avisa. A senha só aceita ASCII imprimível e 8 caracteres ou mais.
- **Tela de carregamento:** o serviço `aic-splash` (sysinit) mostra o logotipo e uma barra de progresso real (serviços iniciados sobre os dos runlevels) no tty2 e troca para ele com `chvt`, porque o OpenRC escreve no tty1; o `aic-console` volta para o tty1 quando assume. O GRUB e o U-Boot só mostram uma linha de marca: trocar isso exige regravar a partição de boot. Valor visual em ASCII puro com `AIC_CONSOLE_ASCII=1` (serial).
- **Comando `aic`:** a porta de entrada das ferramentas do nó, com `--help` em todas: `aic status`, `aic role`, `aic version`, `aic logs [-f]`, `aic update` e `aic passwd`. As ferramentas internas (`aic-console`, `aic-updater`, `aic-update-request`, `aic-splash-run`) também respondem a `--help`, mas não são para uso manual.
- **Biblioteca visual:** `/usr/lib/aic/ui.sh` guarda a paleta, os glifos (com fallback ASCII), o logotipo e as molduras que o console e a tela de carregamento compartilham.
- **Papel do nó:** `aic-role` (por SSH) diz o papel do nó lendo só a configuração local: worker de computação, com ou sem fatias de modelo (`AI_CLUSTER_MODEL_DIR`), ou recuperação (só presença). Mostra também controller, estado do worker, se já há credencial salva, versão e slot. `aic-role --short` dá a linha única que o console mostra em "Papel". A estrutura do cluster e a aprovação do nó são do controller (`ai-cluster topology` e `ai-cluster nodes`), por isso o comando não as afirma e só aponta para elas.
- **Recuperação:** o `init` do initramfs de netboot imprime a marca com "(recuperacao)".

## Limites conhecidos

- **Sem imagem para ARM64.** O x86_64 exige UEFI com Secure Boot desligado e foi validado só em QEMU (sem hardware real, sem recuperação por rede, sem watchdog de placa-mãe); firmware de dispositivo (`linux-firmware`) não vem na imagem.
- **Sem secure boot, dm-verity ou TPM.** A medida mostra que o slot tem o conteúdo esperado, mas um nó comprometido pode informar qualquer valor. Ver [[seguranca]].
- **Sem downgrade pela via normal.** A versão precisa ser maior. Um rollback legítimo exige uma versão nova com o conteúdo antigo.
- **Controller não verifica a assinatura.** Ele guarda a release como recebida. Um pedido inválido ainda faz o nó drenar e voltar.
- **Só uma versão tentada na marca.** Versões diferentes repetem o ciclo de drenagem.
- **Download por HTTP.** A integridade vem do SHA-256 e da assinatura, não do canal.
- **Leitura da medida no boot.** O nó lê 512 MiB do slot antes de subir o worker. Em cartão lento, isso atrasa o início.
- **Chave privada de release fora de controle do cluster.** Quem a tem instala qualquer release em todos os nós.
- **Recuperação por rede sem autenticação.** O servidor TFTP não tem TLS nem lista de revogação.
- **Token de join em claro** em `worker.env` e no script de boot do cartão.
- **Só instalação sob controle do operador.** Não há instalação disparada pelo cluster.

> Planejado: imagem ARM64 e geração automática por arquitetura.
> Planejado: reinstalação de um nó disparada pelo cluster, com a imagem escolhida pelo planejador.
> Planejado: boot assinado, dm-verity e medida ligada a hardware.
> Planejado: minimização de serviços e ajuste de kernel, medido antes de virar padrão.
