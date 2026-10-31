---
title: "ADR-025 — Gatilho de atualização assinada para Linux comum"
aliases: ["ADR-025", "chave de Linux", "gatilho de atualização Linux"]
tags: [sistema/adr, subsistema/operacao]
type: sistema/adr
---

# ADR-025 — Gatilho de atualização assinada para Linux comum

Status: decidido · Data: 2026-10-30 · Voltar para [[decisoes]].

## Contexto

Um worker, controller ou gateway em Linux comum se atualiza com `aic update apply`, que baixa o pacote da origem da instalação e confere **só o SHA-256** que está no `SHA256SUMS` da mesma origem. Quem chega a esse arquivo pode trocar os dois. O AI Cluster OS não tem esse problema: a release é assinada e a chave pública está na imagem ([[adr-018-release-assinada-e-medida-do-slot]]). Para Linux comum não havia assinatura nem chave, e o controller só empurrava atualização aos workers com AI Cluster OS.

## Decisão

1. **Chave própria para Linux comum.** Um par Ed25519 separado da chave que assina o AI Cluster OS (`~/.aic/linux.key`, gerada por `os/image/release-keys.sh linux`). A pública é `os/linux.pub` no repositório. A chave do AI Cluster OS (`release.pub`/`nova.pub`, dentro da imagem) não muda.
2. **Assinatura do `SHA256SUMS`.** `scripts/package-release.sh` gera `SHA256SUMS.sig` (Ed25519 bruta, em base64 numa linha) assinando o arquivo inteiro. Confere-se com `openssl pkeyutl -verify -pubin -rawin` (OpenSSL 3).
3. **Pino na instalação.** `tools.tar.gz` leva a pública como `release.pub`. Os comandos de instalação (`install.sh` e `setup controller|gateway install`, `setup worker join`) a gravam em `/etc/ai-cluster/release.pub`. Nenhum outro comando busca a chave de novo: `update`, `status` e o gatilho usam só o pino.
4. **Confere antes de confiar em qualquer hash.** Vale para: o `install.sh` (logo depois do `tools`, antes de baixar os outros pacotes); `aic update status` e `apply`; o pacote de worker que o convite serve; o `aic-update-linux` (`--verify` e pedido). Sem `release.pub`, a recusa diz como migrar (rodar o `install.sh` desta versão). Assinatura ausente, inválida ou de outra chave: recusa com mensagem, sem baixar nem trocar nada.
5. **Downgrade.** `aic update apply` recusa versão mais antiga que a instalada (ordenação `sort -V`) sem `--force`. Limite: a ordenação é simples, então sufixos como `-rc` não têm semântica própria (ver Limites).
6. **Gatilho do worker Linux pelo mesmo canal do rollout.** O `Release` ganha `linux_signature` (campo 6, Ed25519 do `SHA256SUMS` da release Linux; `url` aponta para o diretório da release). `aic cluster rollout <versão> --url DIR --linux-signature B64` define a release. O worker declara o canal com `AI_CLUSTER_UPDATE_KIND=linux` e usa a assinatura desse canal. Fluxo do worker:
   - o controller anuncia pelo heartbeat, como já fazia com o AI Cluster OS;
   - o worker chama `AI_CLUSTER_UPDATE_CMD=/opt/ai-cluster/bin/aic-update-linux --verify …` **antes de drenar**;
   - se confere, drena, sai do cluster e chama `aic-update-linux …`, que grava o pedido em `state/update-request` (sem root);
   - o unit `ai-cluster-update.path` (root) vê o pedido e roda `setup update linux-apply`: confere de novo, roda `update apply --yes` (com o mesmo rollback já testado) e, se recusar, grava o motivo em `state/update-result`, que o worker mostra e não repete.
7. **Não repetir tentativa.** `AI_CLUSTER_UPDATE_MARK` aponta para `state/update-tried`, como no AI Cluster OS: uma versão que voltou por rollback não é pedida de novo.
8. **Gatilho sem systemd: um vigia root.** Contêiner e OpenRC não têm unit `.path`. Sem systemd, o `setup` sobe um laço root em segundo plano (`setsid sh -c 'while sleep 5; do [ -f state/update-request ] && <setup> update linux-apply; done'`, pid em `/run/ai-cluster-update.pid`) que faz o papel do `ai-cluster-update.path`. O worker se declara atualizável (`AI_CLUSTER_UPDATE_CMD` e `KIND=linux`) junto com o vigia, pelo mesmo `gatilho_linux` do join e do update, então o vigia sobe sempre que a declaração é gravada. Por que essa forma: é a menor que funciona sem init. Um `inotify` ou um supervisor próprio seria código a mais, e o `linux-apply` já confere tudo (assinatura, origem, link simbólico), então o vigia não tem autoridade própria: a pior falha dele é atrasar um pedido. O laço roda o `setup` instalado a cada pedido, não o próprio código, que pode ter sido trocado. O mesmo caminho serve ao OpenRC porque não depende de init; a imagem de teste é Debian sem init, e o OpenRC de verdade não foi exercitado.
9. **Rotação da chave sem reinstalar.** A release leva `release.pub.next` (a chave nova, PEM) e `release.pub.next.sig` (Ed25519 bruta, base64 numa linha, sobre o arquivo inteiro, feita com a chave **pinada**). Os dois entram no `SHA256SUMS`, então a assinatura da release também os cobre. `scripts/rotate-linux-key.sh <nova.pub>` gera o `.sig` com `~/.aic/linux.key` e confere com `os/linux.pub` antes de entregar; `package-release.sh` com `AIC_LINUX_ROTATION=<pasta>` põe os arquivos na release e recusa uma rotação que não confere com `os/linux.pub`. Quem aplica (`update` e o `linux-check` do worker, antes de drenar) confere os hashes contra o `SHA256SUMS` já assinado, confere a rotação com o pino e exige que a chave nova seja Ed25519; qualquer falha recusa sem trocar nada. O pino só muda **depois** que a atualização deu certo: o anterior vai para `release.pub.prev` e a nova, por `mv` de `release.pub.new` (troca atômica). A release da rotação é assinada pela chave antiga; as seguintes, pela nova, e o repositório passa a pinar a nova em `os/linux.pub`.

## Por que o unit root e não sudo

O unit do worker tem `NoNewPrivileges=true`, então `sudo` não funciona para o usuário `ai-cluster`. E a atualização reinicia o próprio serviço do worker: quem a executasse estaria dentro do cgroup que o systemd derruba. Por isso o worker só **pede**, e um unit root separado aplica (o mesmo princípio do `aic-updater` do AI Cluster OS).

## Alternativas

- **Canal de rollout separado por tipo de worker** (uma máquina de estados por canal, dois campos na resposta): descartada. Duplica o canário e o estado para resolver algo que um campo a mais resolve.
- **Reusar a assinatura do AI Cluster OS**: descartada. O requisito é chave separada.
- **Worker roda `aic update apply` com sudo**: descartada (`NoNewPrivileges`, e o restart mata quem chamou).
- **Worker busca o manifesto sozinho**: descartada. Perde o empurrão, e a escolha da versão continuaria sem assinatura.

## Limites

- **Só-Linux em cluster com workers AI Cluster OS.** Resolvido pelo canal declarado ([[adr-028-canal-de-atualizacao-declarado-pelo-worker]]): a release só-Linux não é anunciada a worker de imagem. Worker Linux antigo, sem o campo, ainda conta como imagem até ser atualizado.
- **Controller e gateway em Linux comum não recebem empurrão.** Cada um roda `aic update apply`, agora com a assinatura conferida. Empurrar para eles exigiria um canal de comando no gateway, sem ganho de segurança sobre o pino. Fica em pendência.
- **Sem systemd, o vigia (item 8) não sobrevive a reinício do contêiner**, como os outros serviços sem systemd: volta com o próximo join, `aic update apply` ou `setup update linux-trigger`. Enquanto estiver fora, o worker continua declarado atualizável e pode ser escolhido como canário; o canário pausa em 10 min. Não foi testado com OpenRC de verdade.
- **Confiança no instalador (TOFU).** `install.sh` é TOFU: a chave pinada é a que veio com o `tools` da origem usada na instalação (GitHub, por HTTPS), e quem controla essa origem naquele momento controla o pino. A proteção real vem nas atualizações seguintes: depois do pino, só uma release assinada pela chave pinada atualiza o worker. O worker que entra pelo convite (`curl … | sh`) recebe a chave do servidor do convite, o mesmo trust do install.
- **Origem do pedido.** O worker só aplica pedido cuja URL tem o mesmo esquema e host (origem) do `BASE` gravado na instalação, qualquer que seja o caminho (`latest/download` ou `vX.Y.Z`). Sem isso, um controller ou um pedido forjado poderia fazer o worker buscar em outro host (SSRF).
- **Versões.** Anti-downgrade com `sort -V` não entende `-rc` como pré-lançamento: `1.0.0-rc1` e `1.0.0` são comparados só como texto ordenado. Aceito por enquanto; release de pré-lançamento não deve ser publicada por rollout.
- **OpenSSL.** A conferência usa `openssl pkeyutl -rawin`, que exige OpenSSL 3 ou mais novo. Sem ele a recusa diz isso (não é tratada como assinatura inválida).
- **Rotação (item 9).** Vale só para quem passa pela release que leva a rotação: quem pula essa release tem de reinstalar (o `install.sh` de hoje grava a chave da origem dele). Um `install.sh` de origem antiga, rodado depois, regrava o pino antigo: a instalação continua sendo a única gravação do pino, e a origem dela é de confiança. A lista de revogação continua em pendência (decisão aberta).

## Consequências

- O pacote `worker-linux-<arq>` passa a trazer o `setup` e o `aic-update-linux` (a atualização do worker troca os dois).
- `aic update` aborta antes de baixar qualquer pacote quando a assinatura não confere.
- Servidores instalados antes deste ADR não atualizam até rodar o `install.sh` desta versão (grava o pino); a mensagem de recusa diz isso.
- Os testes de ponta a ponta (`code/tests/phase-u-setup.sh`) cobrem C4 (assinatura no servidor), H3 (gatilho no worker Linux) e J (vigia sem systemd e rotação: válida, assinada por outra chave, pela chave antiga depois da troca).
