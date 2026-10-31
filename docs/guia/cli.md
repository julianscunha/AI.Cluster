---
title: "CLI aic"
aliases: ["aic", "Comandos", "CLI"]
tags: [guia, cli]
type: guia
---

# CLI `aic`

Um só comando para tudo: o cluster, o controller, o gateway, o worker e a atualização. Volte ao [[guia]] para o índice; variáveis em [[configuracao]]. Todo comando aceita `-h` e `--help`. A ajuda e as mensagens saem em português; uma versão em inglês da CLI ainda não existe ([[pendencias]]).

## Gramática

```
aic <recurso> <ação> [alvo] [--opções]
```

- **Recursos:** `cluster` (o cluster visto do controller), `controller`, `gateway` e `worker` (cada um, deste Linux ou do AI Cluster OS), `update` (atualização deste computador) e `version`.
- **`status` é sempre esta máquina:** `aic controller status`, `aic gateway status`, `aic worker status`. A visão do cluster é `aic cluster nodes` (ou `aic cluster dashboard`). Não existe `aic cluster status`.
- **`update` só existe em `aic update`:** `aic update status` consulta (versão instalada × disponível, ou o resultado da última atualização assinada); `aic update apply` instala.
- **Opções com um nome só.** `--controller URL` é o endereço gRPC do controller (porta 50051) e vai depois do grupo (`aic cluster nodes --controller https://ip:50051`); também vale a variável `AI_CLUSTER_CONTROLLER`. `--server IP[:porta]` é o endereço do convite (porta 8099), `--code` é o código do convite, `--minutes` mede janelas, `--lang pt|en` escolhe o idioma do instalador. Um nome antigo não é aceito como apelido.
- **Comandos que falam com o controller** pedem o token de operador (`AI_CLUSTER_OPERATOR_TOKEN`). Exceções: `cluster model manifest` e `cluster model verify`, que trabalham em arquivos locais.
- Endereço sem TLS é recusado, salvo `AI_CLUSTER_INSECURE=1` numa rede fechada.

## Cluster (`aic cluster`)

### Workers

| Comando | O que faz |
|---|---|
| `aic cluster nodes` | Tabela de todos os workers: estado, confiança (`pendente` ou `aprovado`), arquitetura, threads, RAM, scores, saúde, temperatura, swap, classes de armazenamento, `VERSÃO` e `MEDIDA` (`confere` quando a medida bate com a release assinada). `VELOC` é a velocidade estimada que o plano de modelo usa para ordenar os workers; só serve para comparar workers. |
| `aic cluster node show <id>` | Tudo que o controller sabe sobre um worker: capacidade, saúde, confiança e reserva. |
| `aic cluster node approve <id> --fingerprint <fp>` | Aprova um worker pendente para ele receber trabalho. Confere a impressão digital da credencial (`IMPRESSÃO` em `nodes`, também impressa pelo worker ao registrar-se); se o id foi tomado por outro registro, recusa. `--trust-listed` aceita a impressão que o controller mostra agora, sem conferir (anula a proteção). Também tira um worker da quarentena. |
| `aic cluster node retire <id>` | Tira o worker do trabalho **mantendo a identidade**: as unidades em voo voltam à fila, e a credencial e a impressão continuam as mesmas. Volta com `approve` e a impressão de antes. Registra `node_retired`. |
| `aic cluster node revoke <id>` | **Revoga a credencial**: retira a aprovação, troca a credencial, devolve as unidades em voo à fila e registra `node_revoked`. O worker vivo encerra; reiniciado, entra como worker novo e pendente, com credencial nova. O certificado do túnel RPC dele deixa de ser renovado (ADR-026). |
| `aic cluster node remove <id>` | Remove o worker do cluster: o controller o **esquece por inteiro** (registro e confiança) e guarda a credencial dele como lápide. Um worker que ainda roda com ela é **recusado** e não volta sozinho; só volta por um convite novo (`aic worker join`), como pendente (ADR-027). Registra `node_removed`. |

### Instalação com aprovação (ADR-023)

| Comando | O que faz |
|---|---|
| `aic cluster install list` | Aparelhos na espera de instalação: impressão digital, MAC, IP, CPU, RAM, discos e há quanto tempo esperam. |
| `aic cluster install approve <impressão> --disk <disco>` | Aprova a instalação, apagando o disco informado (precisa ser um dos que o aparelho listou). Gera o token de instalação, que só o aparelho dono do segredo recebe. |
| `aic cluster install reject <impressão>` | Tira o aparelho da lista de espera. |
| `aic cluster install open [--minutes N]` | Abre a porta em que os aparelhos se apresentam (1 a 120 min, padrão 15). Ela se fecha sozinha. |
| `aic cluster install close` | Fecha a porta agora. |

### Reservas

Uma reserva retém um worker para uma requisição futura.

| Comando | O que faz |
|---|---|
| `aic cluster reservation list` | Reservas ativas: worker, requisição dona e vencimento. |
| `aic cluster reservation add <worker> <request_id> [--minutes N] [--slots N]` | Reserva o worker. `--minutes 0` (padrão) usa o padrão do controller (60 s). `--slots N` guarda só N slots de trabalho; `0` é o worker inteiro. |
| `aic cluster reservation remove <worker>` | Libera a reserva antes do vencimento. Não tem relação com o `rollout`. |

### Trabalho e diagnóstico

| Comando | O que faz |
|---|---|
| `aic cluster topology` | Estrutura do cluster em árvore: o controller e, abaixo, os workers agrupados por papel. O gateway aparece num ramo à parte. |
| `aic cluster metrics` | Roll-up do cluster: workers por estado, profundidade da fila e tempos das unidades concluídas. |
| `aic cluster events [--limit N] [--follow]` | Eventos recentes, do mais antigo para o mais novo. `--limit` padrão 20; `0` mostra tudo que o controller guarda. `--follow` segue os eventos novos. |
| `aic cluster dashboard [--watch N]` | Painel de uma tela: workers, fila e reservas. `--watch N` repinta a cada N segundos até Ctrl-C; padrão `0`, imprime uma vez. |
| `aic cluster benchmark [--size N] [--repetitions N] [--local] [--min-threads N] [--request-id ID] [--priority N] [--require-attested]` | Despacha multiplicações de matrizes N×N e compara compute com transferência. `--local` roda no próprio processo. `--min-threads` recusa workers com menos threads. `--request-id` enfileira sob um id existente (o de uma reserva, por exemplo). `--priority`: maior sai primeiro. `--require-attested` só aceita workers atestados. Padrões: `--size 256`, `--repetitions 1`. |
| `aic cluster graph [--size N]` | Enfileira o grafo em losango (A, depois B e C, depois D) e mostra os ramos que esperam uns pelos outros. Padrão `--size 128`. |
| `aic cluster request trace <request_id>` | Unidades de uma requisição, com estado, worker em que rodaram e custo. |
| `aic cluster request cancel <request_id>` | Tira da fila as unidades da requisição. As que já estão num worker terminam; o resultado, se vier, é recusado. |

### Modelos

| Comando | O que faz |
|---|---|
| `aic cluster model list` | Modelos da pasta do controller (`AI_CLUSTER_MODELS_DIR`): formato, arquitetura, parâmetros, precisão, tamanho, contexto e se o motor atual os roda (com o motivo, quando não). |
| `aic cluster model check <id> [--ctx N]` | Diz se o cluster comporta o modelo: `OK`, ou `NOK` com o que falta em GB e quantos workers acrescentar. Soma a RAM dos workers aprovados e vivos (no máximo 80% de cada um, menos 128 MiB) contra pesos, cache de contexto e 10% de margem. Sai com código 1 no NOK. |
| `aic cluster model start <id> [--ctx N]` | Sobe o modelo no cluster: confere a RAM, fixa o plano de workers com RPC, cria a chave de API do tenant `default` (mostrada uma vez) e o gateway o carrega. Sai com código 1 no NOK. |
| `aic cluster model stop` | Tira o modelo do gateway. As chaves continuam valendo para o próximo `start`. |
| `aic cluster model status` | O modelo pedido, os workers do plano, o estado que o gateway reportou (CARREGANDO, PRONTO, FALHOU), os tokens/s do benchmark e avisos do plano (worker deixado de fora por versão diferente do rpc-server). |
| `aic cluster model key add <tenant> [--models a,b] [--tokens N] [--class c]` | Cria o tenant e mostra a chave (uma vez; o controller guarda só o hash). `--models` limita os modelos (padrão: todos), `--tokens` dá a cota por janela (0 = sem cota) e `--class` o teto de prioridade (`background`, `batch`, `normal`, `interactive`, `realtime`). |
| `aic cluster model key rotate <tenant>` | Troca a chave; a antiga deixa de valer e os limites ficam. |
| `aic cluster model key remove <tenant>` | Tira o tenant. |
| `aic cluster model key list` | Tenants, com o prefixo da chave e os limites (nunca a chave). |
| `aic cluster model manifest <dir> --name NOME --version VERSÃO` | Calcula o SHA-256 de cada arquivo e grava `manifest.json` na pasta. Local. |
| `aic cluster model verify <dir>` | Confere os arquivos da pasta com o `manifest.json`; sai com erro se faltar o manifest ou se algum arquivo divergir. Local. |

### Atualização da release e convite

| Comando | O que faz |
|---|---|
| `aic cluster rollout <versão> --url URL --sha256 HEX --image-sha256 HEX --signature B64` ou `aic cluster rollout <versão> --url DIR --linux-signature B64` | Define a release que os workers devem rodar. Com `--signature` (AI Cluster OS) o canário atualiza primeiro; com `--linux-signature` (Linux comum, o `SHA256SUMS.sig`) o `--url` é o diretório da release. Pode levar as duas. Os argumentos saem de `os/image/sign-release.sh` e de `scripts/package-release.sh`. O controller confere só o formato; a assinatura é conferida no worker. |
| `aic cluster rollout --cancel` | Encerra o rollout em andamento. |
| `aic cluster invite [--minutes N] [--port P]` | Abre o convite para computadores novos entrarem (instalador). Mostra o código de 8 caracteres e a linha de instalação; fecha ao sair. |

## Controller, gateway e worker deste Linux

| Comando | O que faz |
|---|---|
| `aic controller install [--with-gateway] [--ip IP] [--models-dir DIR] [--yes] [--lang pt\|en]` | Instala o controller neste Linux. Com `--with-gateway`, instala o gateway no mesmo computador. Gera tokens, a CA do túnel e os certificados. |
| `aic controller status` | Estado do controller deste Linux. |
| `aic controller logs [-f] [-n N]` | Registros do controller. `-f` segue; `-n N` mostra só as últimas N linhas. |
| `aic gateway install --server IP[:8099] --code CÓDIGO [--models-dir DIR] [--yes] [--lang pt\|en]` | Instala o gateway de inferência neste Linux, apontado para o controller do convite. |
| `aic gateway status` | Estado do gateway deste Linux. |
| `aic gateway logs [-f] [-n N]` | Registros do gateway. |
| `aic worker join --server IP[:8099] --code CÓDIGO [--name NOME] [--yes] [--lang pt\|en]` | Entra no cluster com um convite: instala o worker neste Linux ou no AI Cluster OS. O nome do worker é o hostname; `--name` escolhe outro (letras, dígitos, ponto e hífen, até 40). Pelo `curl`: `curl -fsSL http://IP:8099/join-CÓDIGO \| sudo sh -s -- --name NOME`. |
| `aic worker status` | Estado do worker deste computador. |
| `aic worker logs [-f] [-n N]` | Registros do worker. |

## Só no AI Cluster OS

Estes comandos respondem que não existem nos Linux comuns.

| Comando | O que faz |
|---|---|
| `aic worker role` | O papel deste worker em uma linha (computação, com fatias de modelo, ou recuperação), lendo só a configuração local. O detalhe (controller, estado, versão, slot) é `aic worker status`. |
| `aic worker change --server IP[:8099] --code CÓDIGO` | Troca o convite (e o controller) do worker. |
| `aic worker change --controller URL --token T` | Troca só o controller, com o token de join. |
| `aic worker change --controller URL --token-file ARQUIVO` | O mesmo, com o token lido de um arquivo: espaços e quebras de linha saem, e o token não aparece no `ps`. Recusa arquivo ausente, vazio ou legível por outros (use `chmod 600`). Não combina com `--token` nem com `--server`. |
| `aic worker passwd [--remove]` | Senha do console local. `--remove` deixa o console sem login. |

## Atualização e versão

| Comando | O que faz |
|---|---|
| `aic update status` | Versão instalada × disponível, ou o resultado da última atualização assinada. |
| `aic update apply [--version V] [--force] [--yes] [--lang pt\|en]` | Atualiza este computador. Em Linux, confere a assinatura do `SHA256SUMS` com a chave pinada antes de qualquer hash, baixa da origem da instalação e troca o que o papel usa; recusa versão mais antiga sem `--force`. No AI Cluster OS, pede ao `aic-updater` que confira agora e mostra o estado (release pendente, slot em teste ou nada); não baixa nada. Ver [[operacao]]. |
| `aic version` | Versão do `aic` e do sistema. |

## Migração dos comandos

Versões anteriores tinham ferramentas separadas (`ai-cluster`, `aic-setup`, `aic-change`, `aic-role`, `aic-passwd`). Elas **não existem mais**, sem apelidos. Use a tabela para trocar os comandos antigos (decisão em [[adr-024-interface-de-comandos-unica-aic]]).

| Era | Agora |
|---|---|
| `ai-cluster nodes`, `ai-cluster status`, `aic-setup status` | `aic cluster nodes` |
| `ai-cluster node show <id>`, `ai-cluster node <id>` | `aic cluster node show <id>` |
| `ai-cluster approve <worker> --fingerprint F` | `aic cluster node approve <worker> --fingerprint F` |
| `ai-cluster revoke`, `retire`, `remove` | `aic cluster node revoke`, `retire`, `remove` |
| `ai-cluster installs` | `aic cluster install list` |
| `ai-cluster approve-install <fp> --disk D` / `reject-install <fp>` | `aic cluster install approve <fp> --disk D` / `reject <fp>` |
| `ai-cluster enrollment open/close` | `aic cluster install open/close` |
| `ai-cluster reserve <worker> <req> --ttl-ms N --slots N` | `aic cluster reservation add <worker> <req> --minutes M --slots N` (`--ttl-ms` em milissegundos vira `--minutes`) |
| `ai-cluster reservations` / `ai-cluster release <worker>` | `aic cluster reservation list` / `reservation remove <worker>` |
| `ai-cluster model …`, `ai-cluster request …` | `aic cluster model …`, `aic cluster request …` |
| `ai-cluster topology`, `metrics`, `events`, `dashboard`, `benchmark`, `graph`, `rollout` | `aic cluster` + o mesmo nome |
| `aic-setup invite` | `aic cluster invite` |
| `aic-setup server` | `aic controller install` |
| `aic-setup gateway …` | `aic gateway install …` |
| `aic-setup worker …`, `os/bootstrap.sh --join-token(-file)` | `aic worker join …`; o `bootstrap.sh` aceita `--token` e `--token-file` |
| `aic-setup update` | `aic update apply` (e `aic update status` para consultar) |
| `aic status`, `aic logs` (AI Cluster OS) | `aic worker status`, `aic worker logs` |
| `aic role`, `aic change`, `aic passwd` | `aic worker role`, `aic worker change`, `aic worker passwd` |
| `aic update` (consultar e aplicar) | `aic update status` e `aic update apply` |
| `aic version` | `aic version` (não mudou) |
