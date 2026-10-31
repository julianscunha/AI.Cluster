---
title: "Operação"
aliases: ["Operar o cluster", "Tokens e papéis", "Solução de problemas"]
tags: [guia, operacao, seguranca]
type: guia
---

# Operação

Como manter um cluster em funcionamento: quem pode fazer o quê, como nós entram e saem, o que fica guardado, como atualizar e o que fazer quando algo não bate. Volte ao [[guia]] para o índice. Comandos da CLI em [[cli]]; variáveis em [[configuracao]].

## Tokens e papéis

Há três segredos de cluster e uma chave de API para aplicações.

| Credencial | Variável | Autoriza | Não autoriza |
|---|---|---|---|
| Join | `AI_CLUSTER_JOIN_TOKEN` | Registrar nó, heartbeat, pedir trabalho, devolver resultado, sair. | Aprovar nó, enfileirar trabalho, reservar, ler eventos administrativos. Recebe `PermissionDenied`. |
| Operador | `AI_CLUSTER_OPERATOR_TOKEN` | Tudo que é administrativo: aprovar e revogar nó, enfileirar trabalho e grafos, reservar, cancelar requisição, rollout, ler resultados. Também lê o estado do cluster. | Ser usado como token de nó. |
| Inferência | `AI_CLUSTER_INFERENCE_TOKEN` (opcional) | Só `ShardMatmul` e o canal de fatias. Permite ao gateway rodar modelos nos workers sem o token de operador. | Qualquer outra chamada. |
| Recuperação | `AI_CLUSTER_RECOVERY_TOKEN` (opcional) | Só aparecer no cluster: registrar, heartbeat, saúde e sair. É o token que o nó guarda em claro para a recuperação pela rede. | Pedir trabalho, entregar resultado, ver nós, release e qualquer chamada de operador. |
| Chave de API | `AI_CLUSTER_API_KEY` ou `AI_CLUSTER_TENANTS` | Usar a API do gateway, conforme o tenant ([[api]]). | Falar com o controller. |

Regras:

- Os três segredos de cluster têm de ser diferentes entre si. O controller e o gateway recusam o que for igual.
- Chave de API de tenant também não pode ser igual a nenhum deles.
- Cada nó recebe, no registro, uma credencial própria de 128 bits. Ela vai em toda chamada do nó (heartbeat, pedido de trabalho, resultado, saída). O operador não precisa conhecê-la.
- Para trocar um token de join ou de operador, mude o valor no controller e reinicie-o. Os workers precisam do novo join token para se registrar de novo. A CLI precisa do novo token de operador.
- Para trocar a chave de API, mude-a no gateway e reinicie-o.

Os tokens entram por variável de ambiente, que aparece para quem inspeciona o processo ou o contêiner (por exemplo, `docker inspect`). O `bootstrap.sh` guarda o ambiente do worker em arquivo 0640 e aceita o token por arquivo. Os arquivos `AI_CLUSTER_NODES` e de credencial ficam em texto claro: restrinja o acesso a eles.

## TLS e mTLS

- Com `AI_CLUSTER_TLS_CERT` e `AI_CLUSTER_TLS_KEY`, o controller serve TLS. Os clientes (workers, CLI, gateway) usam `https://` e verificam o certificado contra `AI_CLUSTER_TLS_CA`.
- Com `AI_CLUSTER_TLS_CLIENT_CA`, o controller exige certificado de cliente assinado por essa CA. Conexão sem certificado, ou com certificado de outra CA, é recusada no handshake, antes de qualquer token ser lido. Cliente precisa de `AI_CLUSTER_TLS_CLIENT_CERT` e `AI_CLUSTER_TLS_CLIENT_KEY`, juntos.
- O certificado de cliente prova que o cliente pertence à CA. Ele não prova qual nó é: o certificado não é ligado ao `node_id`. A identidade de cada nó continua sendo a credencial de registro.
- Não há rotação automática, lista de revogação nem emissão. Troca-se o PEM e reinicia-se.
- O exportador de métricas (`AI_CLUSTER_METRICS_ADDR`) não usa TLS. Ele escuta em loopback por padrão. Se você expô-lo, o token de operador viaja em claro.
- A perna HTTP do gateway é HTTPS quando `AI_CLUSTER_GATEWAY_TLS_CERT` e `_KEY` estão definidos. Sem eles, só escuta fora de loopback com `AI_CLUSTER_INSECURE=1`.

## Aprovação de nós

- Por padrão, um nó novo entra **pendente**: aparece em `aic cluster nodes` com `CONFIANÇA` = `pendente` e não recebe trabalho nem escreve estado.
- `aic cluster node approve <id> --fingerprint <fp>` aprova, conferindo a impressão digital da credencial que o worker imprimiu ao registrar-se (e `nodes` mostra em `IMPRESSÃO`). Se o id foi tomado por outro registro depois de um `revoke`, a impressão muda e a aprovação é recusada. O nó passa a `aprovado` e começa a receber unidades.
- `AI_CLUSTER_AUTO_APPROVE=1` aprova todo nó que se registra. Use só em rede fechada e de teste.
- `aic cluster node revoke <id>` retira a aprovação, troca a credencial do nó, devolve as unidades em voo à fila e registra o evento. O worker revogado encerra com `Unauthenticated`. Se ele for reiniciado, volta pendente. O controller também deixa de renovar o certificado do túnel RPC dele e de anunciá-lo ao gateway (ADR-026); o certificado já emitido segue válido até vencer, sem CRL. Com `AUTO_APPROVE`, a revogação não dura: o nó volta aprovado.
- Três formas de tirar um worker, da mais leve à mais forte: `aic cluster node retire <id>` (sai do trabalho e do plano, mantém a identidade; volta com `approve` e a impressão digital de antes), `aic cluster node revoke <id>` (invalida a credencial: o worker encerra e só volta com um novo join) e `aic cluster node remove <id>` (o controller esquece o nó por inteiro).
- Instalação do AI Cluster OS ([[adr-023-instalacao-com-aprovacao-do-controller]]): com `AI_CLUSTER_ENROLL_ADDR` ligado, o aparelho que dá boot na etapa de espera aparece em `aic cluster install list` com a impressão digital que ele mostra na tela. Confira as duas e rode `aic cluster install approve <impressão> --disk <disco>`; a aprovação apaga o disco. `aic cluster install reject <impressão>` tira da lista. As aprovações aparecem na auditoria como `approve-install` e `reject-install`. No aparelho, `aic-wait http://<controller>:<porta> [--once]` faz o papel da etapa de espera: coleta MAC, CPU, RAM, firmware e discos, mostra a impressão digital na tela e pergunta a cada poucos segundos (Enter pergunta na hora). Aprovado, grava `/run/aic-install.json` com o token e o disco e sai com 0. Para um computador que já tem Linux, `sudo aic cluster invite` mostra também `curl -fsSL http://<servidor>:8099/wait | sudo sh`: o script confere processador, espaço e firmware, baixa só o `aic-wait` do servidor (SHA-256 conferido) e o roda. O convite abre a porta de espera (`aic cluster install open`) pelos minutos dele, avisa no terminal cada aparelho que pediu para instalar e a fecha ao terminar; `aic cluster install close` fecha antes. Sem convite a porta está fechada e o aparelho recebe "instalação fechada". O `aic-wait` ainda não faz parte da imagem de boot, e o uso do token na instalação ainda não existe.
- Um nó aprovado que tenta se registrar de novo sem a credencial dele é recusado até um `revoke`. É o caminho quando o worker perdeu o arquivo de credencial.
- Com `AI_CLUSTER_NODES`, aprovações e credenciais sobrevivem a reinício do controller. Sem ele, todo nó volta a pendente depois de reiniciar.
- Com `AI_CLUSTER_NODE_CREDENTIAL_FILE` no worker, o nó mantém a aprovação depois de reiniciar. Sem o arquivo, ele se registra de novo e fica pendente (ou aprovado, com `AUTO_APPROVE`).

### Quarentena

Um nó que devolve três resultados inválidos seguidos (checksum errado) vai para `QUARANTINED`. Ele continua visível, mas não recebe trabalho. `aic cluster node approve <id> --fingerprint <fp>` tira da quarentena. A quarentena não sobrevive a reinício do controller.

## Entrada, saída e falha

**Entrada.** O worker se registra com o join token, recebe a credencial, mede a capacidade e faz heartbeat a cada 5 s. Depois da aprovação, vira `READY`.

**Saída planejada.** Um worker que recebe SIGTERM ou SIGINT para de pedir trabalho, avisa o controller (`LeaveNode`), devolve as unidades que tiver em voo à fila e encerra. Não há comando de drenagem na CLI: a saída é pelo sinal ao processo. O nó some de `aic cluster nodes`; o controller se lembra das aprovações se `AI_CLUSTER_NODES` estiver configurado.

**Queda.** Sem heartbeat por `AI_CLUSTER_HEARTBEAT_GRACE_MS` (15 s por padrão), o controller marca o nó como `OFFLINE` e devolve as unidades em voo à fila. Cada unidade tem até três tentativas. Quando acabam, a unidade falha e os dependentes também.

**Volta de um nó offline.** O worker tenta pedir trabalho de novo a cada segundo. Depois de 30 recusas seguidas, encerra. Se for reiniciado, registra-se de novo.

**Reservas.** `aic cluster reservation add <nó> <request_id>` retém o nó inteiro para uma requisição (`--slots N` guarda só N slots de trabalho e deixa o resto do nó para outras). Até o vencimento (60 s por padrão; `--minutes` muda), só as unidades dessa requisição vão para o nó. `aic cluster reservation remove <nó>` libera antes. As reservas sobrevivem a reinício do controller.

## Modelos (llama.cpp)

O fluxo, de ponta a ponta (veja [[modelos]] e [[cli]]):

1. Ponha o GGUF em `AI_CLUSTER_MODELS_DIR` (a mesma pasta no controller e no gateway) e confira com `aic cluster model list` (coluna `ROD`) e `aic cluster model check <id>`.
2. `aic cluster model start <id>`: o controller confere a RAM, escolhe os workers e o gateway carrega. Guarde a chave de API que ele mostra (só o hash fica com o controller).
3. `aic cluster model status` mostra o plano, o estado (`CARREGANDO`, `PRONTO`, `FALHOU`) e os tokens/s do benchmark. O Pi leva minutos para receber os tensores; no começo as chamadas esperam até `AI_CLUSTER_LLAMA_WAIT_SECS` (120 s) pelo modelo e, passado o prazo, dão 503 `MODEL_LOADING`.
4. `aic cluster model stop` tira o modelo; a chave continua valendo no próximo `start`.
5. No worker (AI Cluster OS), `aic worker logs -f` mostra o que o nó faz. As linhas `túnel HH:MM:SS` (hora UTC) dizem quando o gateway conectou ou desconectou e, a cada 10 s com tráfego, quantos bytes passaram: é o sinal de que o modelo está usando o nó. As linhas do `rpc-server` do llama.cpp ("Accepted client connection"...) chegam em blocos, porque ele guarda a saída em buffer quando ela vai para arquivo, então podem atrasar ou faltar. `aic worker logs -n 200` mostra mais linhas.
6. Teste a API de outro computador com `code/scripts/aic-test-api.ps1` (Windows, PowerShell 5.1 ou 7, nada para instalar): `powershell -ExecutionPolicy Bypass -File .\code\scripts\aic-test-api.ps1 -Url http://<ip>:8080 -Key <chave>`. Ele confere `/health` e `/v1/models` e mede, por resposta, o tempo até a primeira palavra, o tempo total e os tokens por segundo (`-Runs 5` repete e resume; `-Chat` abre uma conversa com histórico, com `/limpar`, `/medidas` e `/sair`; `-Lang en` mostra em inglês). A chave da instalação está em `/etc/ai-cluster/gateway.env` (`AI_CLUSTER_API_KEY`). Um servidor de arquivos esquecido na porta 8080 do PC (por exemplo `python -m http.server 8080 --bind <ip>`) intercepta o gateway publicado pelo Docker: `/health` responde 404.

Como o controller escolhe: os workers mais rápidos primeiro, **só até a RAM somada comportar o modelo**. Um worker lento entra apenas quando faltaria memória (cada um soma um salto de rede por token). Um worker novo muda o plano só se mudar esse conjunto, e só depois de `AI_CLUSTER_PLAN_STABLE_MS` com a mesma lista.

| Sintoma | Causa provável | O que fazer |
|---|---|---|
| `model start` diz NOK | A RAM somada dos workers com RPC não comporta o modelo, ou não há worker com RPC | Acrescente workers (o NOK diz quantos) ou reduza o contexto (`--ctx`) |
| Status `FALHOU` com "os workers de agora oferecem X GB" | Um worker saiu e a RAM não basta | Traga o worker de volta: o plano volta sozinho |
| Status `FALHOU` com "llama-server saiu (signal: 6)" | O `rpc-server` de um worker caiu (sem memória) ou recusou a conexão | Veja o log do worker; confira a hora do nó e o certificado |
| Worker fica fora do plano | Sem `rpc_port` (RPC desligado), reprovado, sem endereço válido, ou `rpc-server` de outra versão (`aic cluster model status` mostra `AVISO`) | `aic cluster node show <id>`; confira as variáveis `AI_CLUSTER_RPC_*` e `AI_CLUSTER_RPC_VERSION` |
| Muito lento com um Pi no plano | O Pi só entra se a RAM exigir; cada camada nele soma latência | Use um worker maior; um modelo que cabe no PC não precisa do Pi |
| `certificate expired` no túnel | Relógio do nó errado (placa sem RTC) | Corrija a hora (a imagem 11.8 acerta sozinha com DNS e chrony) |

## Eventos e trilha de auditoria

`aic cluster events --limit N` mostra a linha do tempo recente; com `--follow` continua mostrando os eventos novos à medida que acontecem (no máximo 16 assinantes; um evento pode aparecer duas vezes na junção entre a lista e o fluxo). Tipos de evento que o controller grava:

- Nós: `node_registered`, `node_approved`, `node_revoked`, `node_left`, `node_offline`, `node_quarantined`.
- Trabalho: `work_enqueued`, `work_completed`, `work_rejected`, `graph_enqueued`, `request_cancelled`.
- Reservas: `node_reserved`, `reservation_released`.
- Rollout: `rollout` (canário escolhido, pausa, conclusão), `shard_channel_opened`.
- Auditoria: `audit`, com ação, alvo e endereço de quem chamou. Ações registradas: `approve`, `revoke`, `set_release`, `enqueue_work`, `enqueue_graph`, `cancel_request`, `reserve`, `release_reservation`, `approve-install`, `reject-install`. Leituras não entram. Tentativas negadas por token errado também não entram, porque são recusadas antes da ação. Uma ação é registrada antes de ser executada.

Limites:

- Os eventos ficam num anel em memória de 256 entradas. O anel descarta o mais antigo, e eventos comuns não apagam os de auditoria.
- Com `AI_CLUSTER_EVENTS`, cada evento também é acrescentado a um arquivo, sem `fsync`. Numa queda de energia, os últimos podem se perder.
- O arquivo de eventos não tem assinatura e não é à prova de adulteração. Não é um registro de auditoria com garantia legal.
- Identidade de operador é o endereço de rede de quem chamou. O sistema não distingue operadores diferentes que usam o mesmo token.

## Métricas

- `aic cluster metrics` e `aic cluster dashboard` leem o roll-up pelo gRPC, com o token de operador.
- O controller expõe `GET /metrics` em `AI_CLUSTER_METRICS_ADDR` (padrão `127.0.0.1:9101`), no formato Prometheus, com `Authorization: Bearer <token de operador>`. Sem o token, responde `401`. As séries são `ai_cluster_nodes_total`, `ai_cluster_nodes_available`, `ai_cluster_units_pending`, `ai_cluster_units_completed`, `ai_cluster_units_failed`, `ai_cluster_units_blocked`, `ai_cluster_p50_*` e `ai_cluster_p95_*` de latência e compute, `ai_cluster_compute_time_us_total`, `ai_cluster_predicted_compute_time_us_total`, e as séries por nó (`ai_cluster_node_*`: score de compute, saúde, CPU, memória, swap, temperatura, threads).
- As métricas de geração (TTFT, ITL, tokens por segundo, cache KV, cache de respostas e cota por tenant) ficam no gateway, em `GET /metrics` com a chave do tenant. O painel `aic cluster dashboard` não as mostra.
- Não há série temporal dentro do processo. Taxa por segundo se calcula no Prometheus, a partir dos totais.

## Estado, cópias e recuperação

| Componente | Variável | O que guarda | Sobrevive a reinício |
|---|---|---|---|
| Controller | `AI_CLUSTER_NODES` | Nós conhecidos, aprovações e credenciais (texto claro, 0600). | Sim, se o arquivo existir. |
| Controller | `AI_CLUSTER_STATE` | Fila, unidades em voo com o nó que as segura, resultados ainda necessários. Cópia em `<arquivo>.replica`. | Sim. |
| Controller | `AI_CLUSTER_EVENTS` | Linha do tempo de eventos. | Sim, se o arquivo existir. |
| Controller | `AI_CLUSTER_RELEASE_STATE` | Release desejada pelo rollout. | Sim. |
| Worker | `AI_CLUSTER_NODE_CREDENTIAL_FILE` | Credencial do nó (0600). | Sim. |
| Worker | `AI_CLUSTER_STATE_REPLICA` | Cópia do estado do controller, oferecida ao controller quando ele se registra de novo. | Sim. |

Não são guardados: o registro de nós em memória (exceto por `AI_CLUSTER_NODES`), reservas, traces, métricas do gateway, cache de respostas e o cache KV das gerações.

Comportamento na recuperação:

- Reinício do controller com `AI_CLUSTER_STATE`: a fila volta. Uma unidade que estava em voo volta ao mesmo nó. Se o nó não reaparecer em `AI_CLUSTER_HEARTBEAT_GRACE_MS` depois da subida, a unidade volta à fila (ou falha, se as tentativas acabaram).
- Volume do controller perdido, com cópia num nó (`AI_CLUSTER_STATE_REPLICA`): quando o nó se registra de novo, oferece a cópia, e o controller a adota se não tiver trabalho próprio.

Limites:

- A posse de uma unidade só vai ao disco quando outra mutação acontece depois do despacho. Se o controller cair logo após despachar, a unidade volta à fila e pode rodar duas vezes. Ela é recalculada do zero.
- A cópia num nó é a única réplica da fila. Não há fator de replicação nem escolha de nó guardião.
- O estado é um snapshot. Não há histórico de resultados: um resultado sai do estado assim que nenhuma unidade pendente precisa dele.

**Backup.** Copie `AI_CLUSTER_STATE` (e o `.replica`), `AI_CLUSTER_EVENTS` e o arquivo de release. Para `AI_CLUSTER_NODES`, pare o controller antes de copiar. Os arquivos de snapshot são escritos num temporário e depois renomeados, então uma cópia feita durante a operação pega uma versão completa, possivelmente um pouco antiga.

## Atualização

`aic update status` mostra a versão instalada e a disponível (no AI Cluster OS, a última atualização assinada que trouxe a versão atual, registrada quando o slot é confirmado, e a última recusa, se houve). `aic update apply` atualiza este computador; `--version`, `--force` e `--yes` funcionam como no instalador. Os comandos de instalação estão em [[instalacao]].

### Atualização de cada componente

| Componente | Quem atualiza | Como | De onde vem |
|---|---|---|---|
| Controller e gateway (Linux) | Cada máquina, com `aic update apply` | Baixa os pacotes do papel, confere o SHA-256, guarda os binários anteriores em `/opt/ai-cluster/bin/.prev`, troca, acrescenta à configuração só o que a versão nova espera, reinicia e **volta sozinho** se a versão nova não subir. Tokens, certificados, workers aprovados e modelos continuam como estavam. | A origem da instalação (`/etc/ai-cluster/release`) |
| Worker (Linux) | O controller **empurra** a release assinada com a chave de Linux (`aic cluster rollout … --linux-signature`); ou cada máquina, com `aic update apply` | Confere a assinatura do `SHA256SUMS` com a chave pinada (`/etc/ai-cluster/release.pub`) antes de qualquer hash; baixa `worker-linux-<arq>`, confere, troca `ai-cluster-worker`, `ggml-rpc-server`, `aic` e o `setup`, e reinicia o serviço com o mesmo rollback. O pedido do controller passa pelo `aic-update-linux` e é aplicado pelo `ai-cluster-update` (root). Ver [[adr-025-gatilho-de-atualizacao-assinada-linux]]. | A origem do `join` (a assinatura vem com ela) |
| Worker (AI Cluster OS) | O controller **empurra** a release assinada | Imagem em slots A/B, com canário (`aic cluster rollout`, abaixo). `aic update apply` só pede que o `aic-updater` cheque agora, e hoje não faz essa checagem local ([[pendencias]]). | Release assinada pelo operador |

Regras gerais:

- O controller empurra a release assinada para nós com AI Cluster OS (assinatura do AI Cluster OS) e para workers em Linux comum (assinatura de Linux, [[adr-025-gatilho-de-atualizacao-assinada-linux]]). Não empurra para controller nem gateway em Linux comum: cada um roda `aic update apply`.
- Falhas no meio do `aic update apply` desfazem tudo: os binários, o `setup`, o `bootstrap.sh`, o wrapper `aic` e os arquivos `.env` voltam ao que eram (guardados em `/opt/ai-cluster/bin/.prev`; a geração anterior só é substituída depois de a nova subir), o serviço antigo é reiniciado e confirmado, e o comando sai com erro (com texto distinto se o antigo também não subir). O worker confirma o registro no controller depois de reiniciar. Sem `BASE` na release de origem, o update do worker recusa em vez de cair no último lançamento público do GitHub; informe `--version`.

Controller e gateway em Linux comum não recebem empurrão: cada máquina roda `aic update apply`. Por isso `aic cluster nodes` mostra a coluna `VERSÃO` de cada nó, para o operador ver quem está para trás.

### Assinatura da release Linux comum

`scripts/package-release.sh` assina o `SHA256SUMS` com `~/.aic/linux.key` (Ed25519) e grava `SHA256SUMS.sig`; a chave pública (`os/linux.pub`) vai no pacote `tools` como `release.pub`. A instalação grava essa chave em `/etc/ai-cluster/release.pub` e a usa para conferir **toda** atualização, antes de qualquer hash. Só a instalação grava a chave: `update` nunca a busca de novo na origem.

- Assinatura ausente, inválida ou de outra chave: `aic update status` e `apply` recusam com a causa e nada é baixado nem trocado.
- Servidor instalado antes dessa versão (sem `release.pub`): a recusa diz o que fazer. Rode de novo o `install.sh` desta versão (grava a chave) e depois `aic update apply`.
- `aic update apply` recusa versão mais antiga que a instalada sem `--force`.
- Chave nova (rotação): instale a pública nova com o `install.sh` de uma release assinada pela chave nova, em cada máquina. O pino só muda na instalação.
- Trocar o controller de um worker (AI Cluster OS) não é atualização: é `aic worker change` ([[cli]]).

### Worker em Linux sem o `aic`

O `os/bootstrap.sh` continua servindo para pôr o worker num Linux sem o `aic`. Rode-o de novo com o binário novo (veja [[instalacao]]). O binário anterior fica em `bin/ai-cluster-worker.prev`. A versão nova só fica se o worker se registrar dentro de `--health-timeout` segundos (90 por padrão). Senão o anterior volta e o script sai com código 3. Só há uma versão de volta.

### Rollout pelo controller (imagem A/B, armv7 e x86_64)

1. Gere o `.gz` do rootfs (`gzip -n -k os/image/build/rootfs.ext4`; o `build-rootfs.sh` produz só o `.ext4`) e assine a release: `bash os/image/sign-release.sh <versão> <rootfs.gz> <rootfs.ext4> [placa] [chave]`. O script imprime os argumentos do passo seguinte.
2. Publique a release: `aic cluster rollout <versão> --url <url> --sha256 <hex> --image-sha256 <hex> --signature <base64>`. Para os workers em Linux comum, `aic cluster rollout <versão> --url <diretório da release> --linux-signature <base64 de SHA256SUMS.sig>` (o `package-release.sh` imprime a linha pronta). Uma release pode levar as duas assinaturas, e cada nó confere a sua. Sem systemd o worker não recebe o pedido.
3. O controller anuncia a release pelo heartbeat, só para nós que declaram que podem atualizar (`AI_CLUSTER_UPDATE_CMD`). O primeiro nó apto é o canário.
4. O canário baixa a imagem, grava o slot inativo, reinicia, confere a medida do slot e reporta a versão nova. Só então os demais recebem a release. Com `AI_CLUSTER_ROLLOUT_WAVE=N`, no máximo N atualizam ao mesmo tempo.
5. Se o canário não reportar a versão nova em 10 minutos, o rollout pausa (evento `rollout`). Para retomar, publique de novo ou use `aic cluster rollout --cancel`.

Regras que o nó aplica:

- A assinatura Ed25519 é conferida no nó. Assinatura inválida: a release é ignorada, nada é baixado e o slot não muda.
- A URL precisa estar no mesmo host do controller. Caso contrário, é ignorada.
- Versão igual à atual, ou mais antiga pela ordenação de versões, é recusada. Não há downgrade pelo rollout.
- Um nó que já tentou uma versão não a tenta de novo, o que impede laço depois de um rollback.
- Um slot em teste (depois de um boot novo, antes do registro) adia outra atualização.
- Um slot em teste só vira definitivo quando o worker se registra no controller. Depois de duas tentativas de boot que não confirmam, o bootloader volta ao slot confirmado.
- `aic cluster nodes` mostra `MEDIDA confere` quando a medida do slot bate com a release assinada. A medida é reportada pelo próprio nó: detecta slot gravado errado, mas não um nó comprometido que minta.

## Rotação de chave de release

1. `bash os/image/release-keys.sh nova` gera o par `nova.key` e `nova.pub` em `~/.aic`.
2. Monte uma imagem de transição com as duas públicas (`AIC_PUBS="~/.aic/release.pub ~/.aic/nova.pub"`) e assine com a chave antiga. Os nós aceitam assinatura de qualquer chave embutida.
3. Monte a imagem seguinte só com a chave nova e assine com ela. A chave antiga sai dos nós quando essa imagem chegar.

Não há lista de revogação. Uma chave vazada vale até a imagem que a retira chegar a cada nó. Quem tem a chave privada pode instalar qualquer release. Guarde-a fora do repositório e fora do controller.

## Solução de problemas

| Sintoma | Causa provável | O que fazer |
|---|---|---|
| Controller não sobe com "JOIN_TOKEN é obrigatório" ou "OPERATOR_TOKEN é obrigatório". | Token não definido. | Defina os dois, diferentes entre si. |
| Controller não sobe com "sem AI_CLUSTER_TLS_CERT/KEY". | Sem certificado e sem `AI_CLUSTER_INSECURE`. | Passe o par de certificado, ou use `INSECURE` só em rede fechada. |
| Worker ou CLI com "endpoint ... sem TLS". | `http://` sem `AI_CLUSTER_INSECURE`. | Use `https://` com `AI_CLUSTER_TLS_CA`, ou `INSECURE` em rede fechada. |
| Cliente com `Unauthenticated`. | Token de join, de operador ou credencial errado ou ausente. | Confira o valor no cliente e no controller. |
| Cliente com `PermissionDenied` ("exige o token de operador"). | Comando administrativo com o token de join. | Use o token de operador. |
| Registro recusado com `FailedPrecondition`. | Id de nó já aprovado, sem a credencial dele. | `aic cluster node revoke <id>`, depois suba o worker de novo. |
| `approve` recusa com "impressão digital não confere". | O id foi tomado por outro registro depois de um `revoke`, ou a impressão digitada está errada. | Compare com a que o worker imprimiu ao registrar-se e com a coluna `IMPRESSÃO` de `nodes`. |
| Nó fica `pendente`. | Aprovação manual. | `aic cluster node approve <id> --fingerprint <fp>`. |
| Nó fica `QUARANTINED`. | Três resultados inválidos seguidos. | Investigue o nó (checksum, memória, disco); depois `aic cluster node approve <id> --fingerprint <fp>`. |
| Unidade fica `pendente` sem erro. | Nenhum nó vivo e aprovado cumpre o requisito (memória, threads, classe). | `aic cluster node show <id>` para ver o que o nó reporta. Não há erro porque o cluster só não consegue rodar a unidade. |
| Worker encerra depois de algum tempo. | Revogado, ou 30 pedidos de trabalho seguidos recusados (nó offline, drenando ou em quarentena). | Confira `aic cluster nodes`. Reinicie o worker se ele não voltar sozinho. |
| Worker não sobe com erro de manifest. | Pesos do modelo não batem com `manifest.json`. | Restaure os arquivos ou gere o manifest de novo com `aic cluster model manifest <dir> --name <nome> --version <versão>`. |
| Gateway não sobe: "AI_CLUSTER_API_KEY ou AI_CLUSTER_TENANTS é obrigatório". | Nenhuma chave configurada. | Defina uma. |
| Gateway não sobe: "a chave de ... deve ser diferente". | Chave de API igual a um token de cluster. | Gere outra chave. |
| Gateway não sobe: "não é loopback e a perna HTTP não tem TLS". | Endereço de escuta fora de loopback sem `INSECURE`. | Use loopback, ou `INSECURE` numa rede fechada. |
| Gateway responde `503 MODEL_UNAVAILABLE` ou `504 TIMEOUT`. | Nenhum worker vivo, ou nó que sumiu no meio da geração. | `aic cluster nodes`. Confira os `*_SHARDS` do gateway e o `AI_CLUSTER_MODEL_DIR` dos workers. |
| Gateway responde `429 CAPACITY_UNAVAILABLE` no embedding. | Fila de unidades do controller cheia (256). | Espere e tente de novo; reduza a carga. |
| Rollout pausado: "canário ... não chegou a ...". | Canário não reportou a versão em 10 minutos. | Veja o log do canário e o `update-result` no nó. Cancele com `aic cluster rollout --cancel` ou publique de novo. |
| Nó registra "assinatura inválida" ou "versão ... não é mais nova". | Release mal assinada, ou versão antiga. | Assine de novo com a chave certa e use uma versão maior. |
| `aic update` recusa: "não confere com a chave desta instalação". | Release de outra chave, ou `SHA256SUMS` alterado depois de assinado. | Confira a origem (`/etc/ai-cluster/release`). Se a release é sua, confira se `os/linux.pub` é a chave com que ela foi assinada. |
| `aic update` recusa: "anterior à assinatura". | Instalação sem `/etc/ai-cluster/release.pub`. | Rode de novo o `install.sh` desta versão (ele grava a chave) e depois `aic update apply`. |
| Worker Linux: "atualização recusada: …" no log. | O pedido do controller não confere (assinatura, versão ou URL). | O motivo fica em `/opt/ai-cluster/state/update-result`. O worker não tenta de novo essa versão; publique uma release assinada certa. |
| `/metrics` responde `401`. | Token de operador ausente ou errado no bearer. | Use `Authorization: Bearer <token de operador>`. |
| Após reinício, eventos ou fila sumiram. | Variável `AI_CLUSTER_EVENTS` ou `AI_CLUSTER_STATE` não definida. | Defina os caminhos em volume persistente. |
| Após reinício, todos os nós voltaram a `pendente`. | `AI_CLUSTER_NODES` não definido. | Defina o caminho e aprove de novo uma vez. |
