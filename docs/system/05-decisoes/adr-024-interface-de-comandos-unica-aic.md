---
title: "ADR-024 — Uma só interface de comandos: `aic`"
aliases: ["ADR-024", "aic", "interface de comandos"]
tags: [sistema/adr, subsistema/operacao]
type: sistema/adr
---

# ADR-024 — Uma só interface de comandos: `aic`

Status: decidido · Data: 2026-10-30 · Voltar para [[decisoes]].

## Contexto

Os comandos cresceram em ferramentas separadas (`ai-cluster`, `aic-setup`, `aic`, `aic-change`, `aic-role`, `aic-passwd`) com gramáticas diferentes: verbos soltos (`approve`), verbo com hífen (`approve-install`), grupos (`model start`), `status` com quatro sentidos e `update` com dois sentidos opostos (consultar × aplicar). As opções também divergiam (`--join-token`/`--token`, `--server`/`--controller`).

## Decisão

1. **Um único comando de usuário, `aic`**, em toda máquina do AI Cluster (servidor, gateway, worker em Linux e AI Cluster OS). As ferramentas antigas **deixam de existir como comandos**, sem apelidos: `ai-cluster`, `aic-setup`, `aic-change`, `aic-role`, `aic-passwd`, `aic-wait` (este último continua só como programa interno da instalação).
2. **Gramática: `aic <recurso> <ação> [alvo] [--opções]`.** O recurso vem primeiro; a ação é um verbo do vocabulário fixo `list`, `show`, `add`, `remove`, `start`, `stop`, `approve`, `reject`, `open`, `close`, `install`, `join`, `status`, `logs`, `apply`.
3. **`status` significa sempre "esta máquina"**: `aic controller status`, `aic gateway status`, `aic worker status`. A visão do cluster é `aic cluster nodes` e `aic cluster dashboard`. Não há `aic cluster status`.
4. **`update` só em `aic update`**: `aic update status` (versão instalada × disponível, ou o resultado da última atualização assinada) e `aic update apply` (instala).
5. **Opções com um nome só**: `--controller URL` (gRPC, porta 50051); `--server IP[:porta]` (o convite, porta 8099); `--code` (código do convite); `--token` e `--token-file` (token de join); `--minutes` (janelas); `--timeout` em segundos (esperas); `--yes`; `--force`; `--lang pt|en`. Uma opção que existia com outro nome é **removida**, não aliasada.

### Tabela de comandos

| Comando | Faz | Era |
|---|---|---|
| `aic cluster nodes` | tabela de workers (inclui `VELOC` e `VERSÃO`) | `ai-cluster nodes`, `ai-cluster status`, `aic-setup status` |
| `aic cluster node show <id>` | um worker | `node show`, `node <id>` |
| `aic cluster node approve <id> --fingerprint F` | aprova | `approve` |
| `aic cluster node retire <id>` | tira do trabalho, mantém a identidade | `retire` |
| `aic cluster node revoke <id>` | invalida a credencial | `revoke` |
| `aic cluster node remove <id>` | esquece o worker | `remove` |
| `aic cluster install list` | aparelhos em espera | `installs` |
| `aic cluster install approve <impressão> --disk D` / `reject <impressão>` | aprova (apaga o disco) / recusa | `approve-install`, `reject-install` |
| `aic cluster install open [--minutes N]` / `close` | abre/fecha a porta de espera | `enrollment open/close` |
| `aic cluster reservation list` / `add <worker> <req> [--minutes --slots]` / `remove <worker>` | reservas | `reservations`, `reserve`, `release` |
| `aic cluster model list|check|start|stop|status|key|manifest|verify` | modelos | `model …` |
| `aic cluster request trace|cancel <id>` | requisições | `request …` |
| `aic cluster topology|metrics|events|dashboard|benchmark|graph` | leituras e diagnósticos | igual |
| `aic cluster rollout <versão> …` | publica uma release assinada (o controller **empurra** a atualização aos workers com AI Cluster OS) | `rollout` |
| `aic cluster invite [--minutes N] [--port P]` | abre o convite para novos computadores | `aic-setup invite` |
| `aic controller install [--with-gateway] [--ip] [--models-dir]` / `status` / `logs` | controller deste Linux | `aic-setup server` |
| `aic gateway install --server S --code C` / `status` / `logs` | gateway deste Linux | `aic-setup gateway` |
| `aic worker join --server S --code C` / `status` / `logs [-f] [-n N]` | worker deste Linux ou AI Cluster OS | `aic-setup worker`, `aic status/logs` |
| `aic worker role` / `change …` / `passwd [--remove]` | só no AI Cluster OS | `aic role`, `aic change`, `aic passwd` |
| `aic update status` / `apply [--version V] [--force] [--yes]` | atualização deste computador | `aic update`, `aic-setup update` |
| `aic version` | versão do `aic` e do sistema | `aic version` |

### Atualização dos três componentes

- **Controller e gateway (Linux)**: `aic update apply` baixa os pacotes do papel da release de origem, confere o SHA-256, guarda os binários anteriores, troca, acrescenta a configuração nova, reinicia e volta sozinho se não subir.
- **Worker (Linux)**: o `join` grava `/etc/ai-cluster/release` (papel, origem, versão); `aic update apply` baixa `worker-linux-<arq>` da mesma origem, confere, troca `ai-cluster-worker` e `ggml-rpc-server` e reinicia o serviço.
- **Worker (AI Cluster OS)**: a atualização é a assinada, em slots A/B, **empurrada pelo controller** (`aic cluster rollout`); `aic update status` mostra o resultado e `aic update apply` pede ao `aic-updater` que cheque agora.
- **O controller empurra** apenas a release assinada para workers com AI Cluster OS. Para Linux comum não há empurrão: cada máquina roda `aic update apply`, e `aic cluster nodes` mostra a `VERSÃO` de cada uma para o operador ver quem está para trás.

### Serviços sobem sozinhos

Depois de `install`/`join` os serviços ficam habilitados no boot (`systemctl enable --now`, `Restart=always`; OpenRC `rc-update add … default` no AI Cluster OS). Em contêiner sem systemd isso não existe: os processos sobem em segundo plano e não voltam sozinhos depois de reiniciar o contêiner (o instalador avisa).

### Cabeçalho e cores iguais em todo componente

Toda saída que abre para uma pessoa começa com o logotipo da malha de 7 workers e a linha `AI.Cluster <componente> v<versão>`, nas cores do `ai_cluster_common::brand` (título em negrito, logotipo em ciano, apoio em cinza). Rust usa `brand::banner`; os scripts de shell e o PowerShell reproduzem o mesmo desenho e as mesmas cores. Sem terminal (log, pipe) o cabeçalho vira uma linha só.

## Consequências

- Quebra de compatibilidade proposital: scripts que chamam `ai-cluster …` ou `aic-setup …` precisam migrar. Os testes de ponta a ponta, as imagens, os guias e os READMEs migram junto no mesmo commit.
- O binário Rust do CLI passa a se chamar `aic`; o `aic` do AI Cluster OS é o mesmo binário (os programas internos do SO ficam em `/usr/libexec/aic/`, fora do `PATH`).
- Os comandos que instalam ou atualizam continuam sendo scripts de shell (o `aic` os chama), porque precisam rodar antes de existir qualquer binário do cluster.

## Alternativas

- **Manter os nomes antigos como apelidos**: descartada. É o que gerou as sintaxes duplicadas.
- **`aic` só em shell chamando o CLI Rust**: descartada. A ajuda, os erros e o idioma ficariam em duas camadas; com um binário só, o clap cobre ajuda e validação de tudo.
