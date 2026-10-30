---
title: "Configuração"
aliases: ["Variáveis de ambiente", "AI_CLUSTER_*"]
tags: [guia, configuracao]
type: guia
---

# Configuração

Todo componente é configurado por variáveis de ambiente `AI_CLUSTER_*`. Não há arquivo de configuração. Volte ao [[guia]] para o índice.

Convenções:

- Booleano: `1` ou `true` liga. Qualquer outro valor é desligado (exceto onde a tabela diz o contrário).
- Número inválido, em geral, cai no padrão em silêncio. Os valores obrigatórios e de segurança recusam subir, e o erro aparece no log.
- "—" significa que a variável não tem padrão: ou é obrigatória, ou o recurso fica desligado sem ela.

## Controller (`ai-cluster-controller`)

| Variável | Padrão | Efeito |
|---|---|---|
| `AI_CLUSTER_ADDR` | `0.0.0.0:50051` | Endereço de escuta do gRPC (`ip:porta`). |
| `AI_CLUSTER_JOIN_TOKEN` | — (obrigatória) | Token de join: registro e trabalho de nós. Sem ele o controller não sobe. |
| `AI_CLUSTER_OPERATOR_TOKEN` | — (obrigatória) | Token de operador: aprovação, fila, reservas, rollout. Tem de ser diferente do de join. |
| `AI_CLUSTER_INFERENCE_TOKEN` | — | Token de inferência: só `ShardMatmul`. Tem de ser diferente dos outros dois. |
| `AI_CLUSTER_RECOVERY_TOKEN` | — | Token de recuperação pela rede: só registrar, heartbeat, saúde e sair. É o que vai em claro no `boot.scr` do cartão; tem de ser diferente dos outros. |
| `AI_CLUSTER_AUTO_APPROVE` | desligado | `1` ou `true`: todo nó que se registra é aprovado na hora. |
| `AI_CLUSTER_HEARTBEAT_GRACE_MS` | `15000` | Sem heartbeat por esse tempo, o nó vai a `OFFLINE` e suas unidades em voo voltam à fila. |
| `AI_CLUSTER_DATA_DIR` | memória | Diretório (criado se faltar) onde o controller guarda o registro de nós (`nodes`), a fila (`state.bin`) e os eventos (`events.log`). Uma variável só para tudo sobreviver a reinício; as três abaixo, se definidas, valem mais que o diretório. |
| `AI_CLUSTER_NODES` | memória | Arquivo com aprovações e credenciais dos nós. Texto claro, modo 0600. Sem ele, o registro some no reinício. |
| `AI_CLUSTER_STATE` | memória | Arquivo do snapshot da fila, das unidades em voo e dos resultados ainda necessários. A cópia fica em `<arquivo>.replica`, ao lado. |
| `AI_CLUSTER_EVENTS` | memória | Arquivo da linha do tempo de eventos. Sobrevive a reinício; é reescrito com as últimas 256 linhas ao passar de 1024. |
| `AI_CLUSTER_TRACES` | memória | Arquivo do histórico de unidades concluídas (traces e roll-up). Sobrevive a reinício; guarda as últimas 100 mil unidades e gira para `.1` ao passar disso. Com `AI_CLUSTER_DATA_DIR` o padrão é `traces.log`. |
| `AI_CLUSTER_EVENTS_SYSLOG` | desligado | `host:porta` de um coletor syslog UDP (RFC 5424). Cada evento novo sai como um datagrama; os eventos `audit` levam o IP de quem agiu, em texto claro. |
| `AI_CLUSTER_METRICS_ADDR` | `127.0.0.1:9101` | Endereço do exportador Prometheus (`GET /metrics`). Exige o token de operador como bearer. |
| `AI_CLUSTER_TLS_CERT` | — | Certificado PEM do servidor. Vem com `AI_CLUSTER_TLS_KEY`. |
| `AI_CLUSTER_TLS_KEY` | — | Chave PEM do servidor. Vem com `AI_CLUSTER_TLS_CERT`. |
| `AI_CLUSTER_TLS_CLIENT_CA` | — | CA PEM que assina certificados de cliente. Ativa o mTLS: sem certificado dessa CA, a conexão é recusada no handshake. Exige o par de servidor. |
| `AI_CLUSTER_INSECURE` | desligado | `1` permite subir sem TLS (tokens em claro). Com certificado presente, o certificado prevalece. |
| `AI_CLUSTER_RELEASE_STATE` | memória | Arquivo de uma linha com a release desejada do rollout. Sobrevive a reinício. |
| `AI_CLUSTER_MODELS_DIR` | — | Pasta de modelos do controller (GGUF ou pastas com safetensors). `ai-cluster model list` lê os cabeçalhos e diz o que é cada modelo e se o motor atual o roda. Sem ela, a lista vem vazia. |
| `AI_CLUSTER_MODELS_STATE` | `$AI_CLUSTER_DATA_DIR/models.state` | Arquivo com o modelo pedido, a geração e o hash das chaves de API. Sem ele (e sem `AI_CLUSTER_DATA_DIR`) o pedido não sobrevive a um reinício. |
| `AI_CLUSTER_PLAN_STABLE_MS` | `30000` | Quanto a lista de workers com RPC precisa ficar igual antes de o plano do modelo mudar (e a geração subir). |
| `AI_CLUSTER_ROLLOUT_WAVE` | `0` (todos) | Quantos nós atualizam ao mesmo tempo depois do canário. |
| `AI_CLUSTER_PLACEMENT` | ligado | `off`, `0` ou `false` desliga o placement por custo e despacha na ordem de chegada. Serve para comparação. |
| `AI_CLUSTER_SHARD_TIMEOUT_MS` | `8000` | Prazo de uma chamada `ShardMatmul` (motor próprio), de 1000 a 300000. Nós lentos pedem mais. |
| `AI_CLUSTER_PLACEMENT_TOLERANCE` | `0.10` | Faixa de histerese do custo, como fração do custo do nó mais barato. Negativo vira `0`. |
| `AI_CLUSTER_UNIT_SETUP_US` | `600` | Custo fixo por unidade, em microssegundos, usado na previsão e no custo. Negativo vira `0`. |

## Worker (`ai-cluster-worker`)

| Variável | Padrão | Efeito |
|---|---|---|
| `AI_CLUSTER_CONTROLLER` | `http://127.0.0.1:50051` | Endpoint do controller. `http://` exige `AI_CLUSTER_INSECURE`; `https://` exige `AI_CLUSTER_TLS_CA`. |
| `AI_CLUSTER_JOIN_TOKEN` | — | Token de join. Sem ele o controller recusa o registro. |
| `AI_CLUSTER_NODE_ID` | nome do host (`/etc/hostname`), ou `worker` | Id do nó no cluster. |
| `AI_CLUSTER_NODE_CREDENTIAL_FILE` | memória | Onde guardar a credencial recebida no registro (modo 0600). Com ele, o nó mantém a aprovação depois de reiniciar. |
| `AI_CLUSTER_STATE_REPLICA` | não guarda cópia | Arquivo onde o nó guarda uma cópia do estado do controller. Serve para recuperar a fila se o volume do controller morrer. |
| `AI_CLUSTER_WORK_SLOTS` | `1` (1 a 64) | Unidades executadas ao mesmo tempo. Os cores reportados são divididos entre elas. |
| `AI_CLUSTER_MODEL_DIR` | — | Raiz dos modelos. Ativa o canal de fatias residentes. Cada diretório com `manifest.json` é verificado na subida; divergência encerra o worker. |
| `AI_CLUSTER_SHARD_CACHE_MB` | `2048` | Memória máxima das fatias residentes (despejo LRU). O worker reporta o valor, e o gateway recusa na subida um modelo que não cabe ("faltam X GB"). |
| `AI_CLUSTER_RPC_SERVER_BIN` | — | Caminho do `ggml-rpc-server` (llama.cpp). Liga o RPC: o worker o roda como filho supervisionado, escutando só em 127.0.0.1, atrás de um túnel mTLS. Exige `AI_CLUSTER_RPC_TLS_CERT`, `_KEY` e `_CLIENT_CA`; sem eles o nó segue sem RPC. |
| `AI_CLUSTER_RPC_TLS_CERT` / `AI_CLUSTER_RPC_TLS_KEY` | — | Certificado (SAN `aic-worker`, ou o nome combinado com o gateway) e chave do túnel, em PEM. |
| `AI_CLUSTER_RPC_TLS_CLIENT_CA` | — | CA que assina o certificado do gateway. Sem certificado de cliente dessa CA o handshake falha. **Use uma CA que só assine o gateway**: o túnel aceita qualquer certificado dela, e o `rpc-server` executa o que o cliente manda. |
| `AI_CLUSTER_RPC_CLIENT_NAME` | `aic-gateway` | Nome no SAN que o certificado do **cliente** do túnel precisa ter. O certificado de um worker (SAN `aic-worker`) é da mesma CA, mas não vale como cliente: um worker comprometido não abre o `rpc-server` de outro. |
| `AI_CLUSTER_RPC_LISTEN` | `0.0.0.0:50061` | Onde o túnel escuta; é a única porta aberta para a rede. |
| `AI_CLUSTER_RPC_LOCAL_PORT` | `50062` | Porta do `rpc-server` em loopback. |
| `AI_CLUSTER_RPC_CACHE` | automático com `AI_CLUSTER_RPC_CACHE_DIR` | `1`/`true` liga e qualquer outro valor desliga o `rpc-server -c` (cache dos tensores em disco). Sem a variável e com `AI_CLUSTER_RPC_CACHE_DIR`, o worker mede o disco e liga o cache se o disco lê pelo menos 1,5 vez a velocidade do enlace (o Orange Pi com SD de 22 MB/s e enlace de 100 Mbit liga). O cache só guarda tensores grandes. Medido no Orange Pi (SD, 100 Mbit) com um modelo de 0,37 GB, que guardou 138 MB: a 1ª carga levou 81 s (grava o cache a ~5 MB/s) e a 2ª, 42 s, contra 51 a 67 s sem cache; o ganho cresce com o tamanho dos tensores. |
| `AI_CLUSTER_RPC_CACHE_MAX_MB` | `2048` | Teto do cache em disco do `rpc-server`: ao subir, o worker apaga os arquivos mais antigos até caber. |
| `AI_CLUSTER_RPC_CACHE_DIR` | diretório do usuário do llama.cpp | Onde o `rpc-server` guarda o cache (`LLAMA_CACHE`). No Compose é o volume `worker-cache` em `/cache`; fora de contêiner, aponte para o disco real do nó, não para tmpfs. |
| `AI_CLUSTER_RPC_ADVERTISE_HOST` / `AI_CLUSTER_RPC_VERSION` | — | Endereço que o gateway usa e versão do llama.cpp com o patch; o nó anuncia os dois. |
| `AI_CLUSTER_REQUIRE_MANIFEST` | desligado | `1` ou `true`: diretório de modelo sem `manifest.json` é erro. |
| `AI_CLUSTER_UPDATE_CMD` | — | Comando de atualização, executado sem shell com versão, URL, SHA-256 da release, SHA-256 da imagem e assinatura. Sem ele o nó não participa do rollout. |
| `AI_CLUSTER_UPDATE_MARK` | `/data/state/update-tried` | Arquivo com a última versão tentada. Impede repetir a atualização depois de um rollback. |
| `AI_CLUSTER_RELEASE_FILE` | `/etc/aic-release` | Arquivo com a versão que o nó roda. Sem ele, vale a versão do binário. |
| `AI_CLUSTER_MEASURE_FILE` | `/run/aic-measure` | Arquivo com a medida (SHA-256) do slot ativo, reportada ao controller. |
| `AI_CLUSTER_CPU_THREADS` | threads disponíveis | Número de threads reportado no lugar do real. |
| `AI_CLUSTER_HEARTBEAT_SECS` | `5` | Intervalo entre batidas, de 1 a 10 s. Menos que o padrão encurta o tempo até o aviso de cancelamento chegar ao nó. |
| `AI_CLUSTER_MEMORY_BYTES` | `MemTotal` | Memória reportada no lugar da real. |
| `AI_CLUSTER_COMPUTE_SCORE` | medido | Fixa o score de compute (maior que zero). O nó deixa de remedir. |
| `AI_CLUSTER_NETWORK_SCORE` | medido na interface | Fixa o score de rede (maior que zero). Zero ou inválido não fixa: o nó mede. |
| `AI_CLUSTER_STORAGE_CLASSES` | detectado em `/sys/block` | Lista de classes separadas por vírgula (`HOT`, `WARM`, `COLD`) no lugar da detecção. |
| `AI_CLUSTER_STORAGE_PROBE_DIR` | primeiro de: diretório temporário, `/var/tmp`, diretório de trabalho do worker que não seja RAM | Onde gravar as sondas de disco. |
| `AI_CLUSTER_STORAGE_HEALTH` | medido | Fixa a saúde do armazenamento entre 0 e 1 e desliga a sonda. |
| `AI_CLUSTER_RECOMPUTE_INTERVAL_SECS` | `60` | Intervalo de remedição quando ocioso. `0` remede a cada passada ociosa. |
| `AI_CLUSTER_TEMPERATURE_C` | sensor do sistema | Fixa a temperatura, em °C. |
| `AI_CLUSTER_BATTERY` | `/sys/class/power_supply` | Simula a bateria no formato `<percentual>:<charging\|discharging>`. |
| `AI_CLUSTER_WORK_DELAY_MS` | `0` | Teste: segura a unidade antes de calcular. |
| `AI_CLUSTER_CORRUPT_RESULTS` | desligado | Teste: qualquer valor presente corrompe o checksum enviado. Serve para exercitar a quarentena. |

## Gateway (`ai-cluster-gateway`)

| Variável | Padrão | Efeito |
|---|---|---|
| `AI_CLUSTER_GATEWAY_STATE` | `AI_CLUSTER_DATA_DIR/gateway-<id>.state` | Arquivo onde o gateway grava os contadores de inferência para sobreviverem ao reinício. Sem ele e sem o diretório de dados, os contadores partem de zero. |
| `AI_CLUSTER_GATEWAY_ID` | `gateway-<endereço>` | Nome com que o gateway aparece em `ai-cluster topology`. |
| `AI_CLUSTER_GATEWAY_URL` | `http://<endereço>` | URL pela qual os clientes chegam ao gateway, mostrada na topologia. |
| `AI_CLUSTER_GATEWAY_ADDR` | `127.0.0.1:8080` | Endereço HTTP. Fora de loopback exige TLS (as duas variáveis abaixo) ou `AI_CLUSTER_INSECURE`. |
| `AI_CLUSTER_GATEWAY_TLS_CERT` / `AI_CLUSTER_GATEWAY_TLS_KEY` | — | Certificado (cadeia) e chave privada em PEM: a perna HTTP passa a ser HTTPS (HTTP/1.1) e o texto puro deixa de ser atendido. Vêm juntas. |
| `AI_CLUSTER_API_KEY` | — | Chave do tenant `default`: todos os modelos, sem cota. Obrigatória se `AI_CLUSTER_TENANTS` estiver vazio. |
| `AI_CLUSTER_TENANTS` | — | Tenants no formato `nome=chave[,models=a\|b][,tokens=N][,class=c];...`. `models` limita os modelos, `tokens` dá cota por janela, `class` é a classe máxima. |
| `AI_CLUSTER_QUOTA_WINDOW_SECS` | `3600` | Janela da cota de tokens. |
| `AI_CLUSTER_QUEUE_MAX` | `8` | Pedidos de chat que podem esperar a vez na geração. |
| `AI_CLUSTER_QUEUE_WAIT_SECS` | `10` | Tempo máximo de espera na fila de geração. |
| `AI_CLUSTER_CACHE_ENTRIES` | `256` | Entradas do cache exato de respostas. `0` desliga o cache. |
| `AI_CLUSTER_CONTROLLER` | `http://127.0.0.1:50051` | Endpoint do controller. |
| `AI_CLUSTER_OPERATOR_TOKEN` | — | Token de operador. Necessário para embeddings e para a conexão de operador. |
| `AI_CLUSTER_INFERENCE_TOKEN` | — | Token de inferência, para modelos de linguagem com fatias nos workers. Pelo menos um dos dois tokens é obrigatório. |
| `AI_CLUSTER_REQUIRE_MANIFEST` | desligado | Como no worker: exige `manifest.json` nos diretórios de modelo. |
| `AI_CLUSTER_GPT2_DIR` | — | Liga o modelo `gpt2` com os pesos neste diretório (verificados pelo manifest, se houver). |
| `AI_CLUSTER_GPT2_SHARDS` | — | Ids de nó separados por vírgula. As projeções do `gpt2` passam a rodar nesses workers. |
| `AI_CLUSTER_GPT2_SUBDIR` | `gpt2` | Subdiretório do modelo sob `AI_CLUSTER_MODEL_DIR` nos workers. |
| `AI_CLUSTER_SMOLLM2_DIR` | — | Pasta com os pesos do `smollm2-135m` (base, sem template de chat) neste servidor; liga o modelo. |
| `AI_CLUSTER_SMOLLM2_SHARDS` | — | Nós que guardam as projeções do `smollm2-135m`. |
| `AI_CLUSTER_SMOLLM2_SUBDIR` | `smollm2` | Pasta dos pesos dentro de `AI_CLUSTER_MODEL_DIR` nos workers (só para fatias). |
| `AI_CLUSTER_SMOLLM2_INSTRUCT_DIR` | — | Liga o modelo `smollm2-135m-instruct` (com template ChatML). |
| `AI_CLUSTER_SMOLLM2_INSTRUCT_SHARDS` | — | Nós que guardam as projeções do `smollm2-135m-instruct`. |
| `AI_CLUSTER_MODELS_DIR` (gateway) | — | A mesma pasta de modelos do controller, vista pelo gateway. Com ela, o gateway aplica o modelo que o controller pede (`model start`). `AI_CLUSTER_LLAMA_MODEL` fixo vale mais que o controller. |
| `AI_CLUSTER_LLAMA_MODEL` | — | Caminho de um GGUF. Liga o modelo servido pelo `llama-server` do llama.cpp (ADR-022): o gateway é o nó principal. |
| `AI_CLUSTER_LLAMA_MODEL_ID` | nome do arquivo | Id do modelo na API (`GET /v1/models`). |
| `AI_CLUSTER_LLAMA_SERVER_BIN` | `llama-server` | Caminho do `llama-server`. |
| `AI_CLUSTER_LLAMA_RPC_ENDPOINTS` | — | `host:porta` dos túneis dos workers, separados por vírgula. O `llama-server` divide as camadas com eles. Sem a lista, o modelo roda só na CPU do gateway. |
| `AI_CLUSTER_LLAMA_TLS_CA` / `_CERT` / `_KEY` | — | CA que assina os workers e o certificado de cliente do gateway. Obrigatórios com endpoints: o RPC não vai em texto puro. |
| `AI_CLUSTER_LLAMA_RPC_SERVER_NAME` | `aic-worker` | Nome que o certificado do worker precisa ter (SAN). Com `{node_id}` (por exemplo `aic-worker-{node_id}`) cada worker precisa de um certificado com o nome do próprio nó: um certificado roubado não serve a outro worker. |
| `AI_CLUSTER_RPC_VERSION` | — | Versão do llama.cpp com o patch (`llama.cpp-8ca4060+armv7`). O worker a anuncia e o gateway a reporta; o controller deixa fora do plano o worker de versão diferente, porque o protocolo do RPC muda entre commits. As imagens e o Compose já a definem. |
| `AI_CLUSTER_LLAMA_SPLIT` | automático | Proporção de camadas por worker, na ordem dos endpoints (`1,4` dá um quinto ao primeiro). Vale mais que a divisão do controller. Para experimentos. |
| `AI_CLUSTER_STATE_WRITE_MS` | `250` (0 = a cada mutação) | Intervalo mínimo entre gravações do snapshot do controller; um flush por segundo grava o que ficou. Poupa SD e disco fraco. O desligamento limpo (`docker stop`, Ctrl+C) grava antes de sair; um crash perde no máximo ~1,3 s de mutações. |
| `AI_CLUSTER_LLAMA_WAIT_SECS` | `120` (0 a 600) | Quanto um pedido espera o modelo carregar (recarga, queda de worker) antes de levar 503 `MODEL_LOADING`; também limita a espera pela recarga antes de refazer um pedido cortado. |
| `AI_CLUSTER_LLAMA_SLOTS` | `1` (1 a 16) | Gerações ao mesmo tempo no `llama-server` (`-np`) e vagas da fila do gateway. O contexto de cada vaga é `AI_CLUSTER_LLAMA_CTX` (o `-c` do servidor é o total), e o cache KV cresce com as vagas (o gateway informa as vagas ao controller, que as usa no OK/NOK; sem gateway conectado, o controller lê a mesma variável do seu ambiente). Mais vagas aumentam a vazão total e alongam cada resposta. |
| `AI_CLUSTER_LLAMA_CTX` / `AI_CLUSTER_LLAMA_NGL` | `4096` / `99` | Contexto (`-c`) e camadas enviadas aos workers (`-ngl`). Uma vaga só (`-np 1`). |
| `AI_CLUSTER_LLAMA_PORT` / `AI_CLUSTER_LLAMA_TUNNEL_BASE` | `18080` / `18100` | Portas locais do `llama-server` e do primeiro túnel (um por worker). |
| `AI_CLUSTER_SMOLLM2_INSTRUCT_SUBDIR` | `smollm2-instruct` | Subdiretório dos pesos nos workers. |

`AI_CLUSTER_JOIN_TOKEN` também é lida pelo gateway, só para garantir que nenhuma chave de API seja igual a ele.

## Cliente de controller (worker, CLI e gateway)

Estas variáveis valem para toda conexão com o controller:

| Variável | Padrão | Efeito |
|---|---|---|
| `AI_CLUSTER_TLS_CA` | — | CA PEM que assina o certificado do controller. Obrigatória com `https://`. |
| `AI_CLUSTER_TLS_DOMAIN` | — | Nome a verificar no certificado, quando o endpoint é um IP ou um alias. |
| `AI_CLUSTER_TLS_CLIENT_CERT` | — | Certificado de cliente para mTLS. Vem com `AI_CLUSTER_TLS_CLIENT_KEY`. |
| `AI_CLUSTER_TLS_CLIENT_KEY` | — | Chave do certificado de cliente. |
| `AI_CLUSTER_INSECURE` | desligado | `1` permite `http://` sem TLS. |

## CLI (`ai-cluster`)

| Variável | Padrão | Efeito |
|---|---|---|
| `AI_CLUSTER_CONTROLLER` | `http://127.0.0.1:50051` | Mesmo que a flag global `--controller`. |
| `AI_CLUSTER_OPERATOR_TOKEN` | — | Token de operador. Todos os comandos que falam com o controller usam. |

As variáveis de TLS e de `AI_CLUSTER_INSECURE` da tabela de cliente também valem para a CLI.

## Scripts de build e imagem

Estas não são lidas pelos binários.

| Variável | Onde | Efeito |
|---|---|---|
| `TARGET` | `scripts/build-armv7.sh` | Alvo de compilação. Para a imagem Alpine use `armv7-unknown-linux-musleabihf`. |
| `AIC_KEYS` | `os/image/*.sh` | Diretório das chaves de release (padrão `~/.aic`). |
| `AIC_PUBS` | `os/image/build-rootfs.sh` | Chaves públicas embutidas na imagem, separadas por espaço. Mais de uma permite a transição de rotação. |
| `AIC_RELEASE` | `os/image/build-rootfs.sh` | Versão gravada em `/etc/aic-release` (padrão `dev`). |
| `CARD_BYTES` | `os/image/mk-disk-image.sh` | Tamanho do cartão para o layout A/B (padrão 3.933.208.576 bytes). |
| `AIC_RUST_IMAGE` | `scripts/dev-cluster.sh` | Imagem Rust usada pelo cluster de desenvolvimento (padrão `rust:1-bookworm`). |
| `AIC_SSH_PASSWORD` | `os/image/install-disk.py` | Senha SSH do nó em recuperação. Não vai na linha de comando. |

## Segredos em arquivo

Cada token e a chave da API aceitam a variável `<NOME>_FILE` com o caminho de um arquivo que guarda o valor (`AI_CLUSTER_JOIN_TOKEN_FILE`, `AI_CLUSTER_OPERATOR_TOKEN_FILE`, `AI_CLUSTER_INFERENCE_TOKEN_FILE`, `AI_CLUSTER_RECOVERY_TOKEN_FILE`, `AI_CLUSTER_API_KEY_FILE`). Serve para secrets do Docker e do Kubernetes e mantém o valor fora de `docker inspect` e de `/proc/<pid>/environ`. O arquivo vence a variável. No Unix ele precisa ter permissão `0600`; arquivo ausente, vazio ou aberto demais conta como "sem segredo" (e avisa), nunca cai para a variável.

## Quem pode tomar um id

- **Nó:** quem tem só o token de join não toma o `node_id` de um nó aprovado nem o de um pendente que ainda responde; um nó pendente sem sinal fica livre. `ai-cluster revoke` também libera o id.
- **Gateway:** enquanto um gateway está vivo (sinal nos últimos 45 s), outro endereço IP não se apresenta com o mesmo id (`permission_denied`). O token de inferência é compartilhado, então o endereço é o que amarra o id.

**Checksum de modelo:** coloque `<arquivo>.gguf.sha256` (saída do `sha256sum`) ao lado do GGUF e o `model start` confere o hash antes de planejar (num GGUF dividido, cada parte pode ter o seu); hash diferente recusa o modelo. Sem o arquivo, não confere.
