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

## Por que o unit root e não sudo

O unit do worker tem `NoNewPrivileges=true`, então `sudo` não funciona para o usuário `ai-cluster`. E a atualização reinicia o próprio serviço do worker: quem a executasse estaria dentro do cgroup que o systemd derruba. Por isso o worker só **pede**, e um unit root separado aplica (o mesmo princípio do `aic-updater` do AI Cluster OS).

## Alternativas

- **Canal de rollout separado por tipo de worker** (uma máquina de estados por canal, dois campos na resposta): descartada. Duplica o canário e o estado para resolver algo que um campo a mais resolve.
- **Reusar a assinatura do AI Cluster OS**: descartada. O requisito é chave separada.
- **Worker roda `aic update apply` com sudo**: descartada (`NoNewPrivileges`, e o restart mata quem chamou).
- **Worker busca o manifesto sozinho**: descartada. Perde o empurrão, e a escolha da versão continuaria sem assinatura.

## Limites

- **Só-Linux em cluster com workers AI Cluster OS.** Uma release só com `linux_signature` é recusada pelos workers AI Cluster OS, e se um deles for canário o rollout pausa em 10 min. Quem tem os dois tipos de worker publica as duas assinaturas.
- **Controller e gateway em Linux comum não recebem empurrão.** Cada um roda `aic update apply`, agora com a assinatura conferida. Empurrar para eles exigiria um canal de comando no gateway, sem ganho de segurança sobre o pino. Fica em pendência.
- **Sem systemd não há gatilho**: o worker avisa, não se declara atualizável (fica fora do rollout, para não virar canário que nunca reporta) e a atualização é manual (`aic update apply`).
- **Confiança no instalador (TOFU).** `install.sh` é TOFU: a chave pinada é a que veio com o `tools` da origem usada na instalação (GitHub, por HTTPS), e quem controla essa origem naquele momento controla o pino. A proteção real vem nas atualizações seguintes: depois do pino, só uma release assinada pela chave pinada atualiza o worker. O worker que entra pelo convite (`curl … | sh`) recebe a chave do servidor do convite, o mesmo trust do install.
- **Origem do pedido.** O worker só aplica pedido cuja URL tem o mesmo esquema e host (origem) do `BASE` gravado na instalação, qualquer que seja o caminho (`latest/download` ou `vX.Y.Z`). Sem isso, um controller ou um pedido forjado poderia fazer o worker buscar em outro host (SSRF).
- **Versões.** Anti-downgrade com `sort -V` não entende `-rc` como pré-lançamento: `1.0.0-rc1` e `1.0.0` são comparados só como texto ordenado. Aceito por enquanto; release de pré-lançamento não deve ser publicada por rollout.
- **OpenSSL.** A conferência usa `openssl pkeyutl -rawin`, que exige OpenSSL 3 ou mais novo. Sem ele a recusa diz isso (não é tratada como assinatura inválida).
- **Rotação.** Trocar a chave exige reinstalar (ou rodar o `install.sh` novo) em cada máquina, porque o pino só muda na instalação. A lista de revogação continua em pendência.

## Consequências

- O pacote `worker-linux-<arq>` passa a trazer o `setup` e o `aic-update-linux` (a atualização do worker troca os dois).
- `aic update` aborta antes de baixar qualquer pacote quando a assinatura não confere.
- Servidores instalados antes deste ADR não atualizam até rodar o `install.sh` desta versão (grava o pino); a mensagem de recusa diz isso.
- Os testes de ponta a ponta (`code/tests/phase-u-setup.sh`) cobrem C4 (assinatura no servidor) e H3 (gatilho no worker Linux).
