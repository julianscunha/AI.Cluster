---
title: "Segurança e confiança"
aliases: ["Segurança", "Security & Trust", "Confiança"]
tags: [sistema/subsistema/plataforma]
type: sistema/subsistema
---

# Segurança e confiança

Segurança cobre quem é cada participante do cluster, o que cada um pode fazer, como a conexão é protegida, como um nó é aceito, suspenso e retirado, como uma release é verificada e como as ações deixam rastro. Voltar para [[plataforma]].

## Princípios

- **Falha fechada.** Controller sem token de join ou de operador não sobe. Token vazio no servidor não aceita ninguém. Gateway sem chave de API não sobe.
- **Menor privilégio por papel.** Quem registra e trabalha (nó) não aprova. Quem aprova (operador) não age como nó. Quem só faz inferência não toca no plano de controle.
- **Não confiar por padrão.** Nó novo nasce pendente e não recebe trabalho até ser aprovado.
- **Identidade vem do controller.** O que o nó declara sobre si (`node_id`, `device_id`) não é prova. A prova é a credencial que o controller emitiu.

## Identidades

| Identidade | Como é provada | Limite |
|---|---|---|
| Física (dispositivo) | Não há. O `device_id` é texto que o worker envia. | Não verificada. |
| Lógica (nó) | Credencial de 128 bits, emitida no registro, enviada em toda chamada. | Só prova posse da credencial, não o hardware. |
| Certificado de cliente (mTLS) | Certificado assinado pela CA do cluster, opcional. | Prova que o peer pertence ao cluster, mas não está ligado ao `node_id`. |

## Configuração de segredos

| Variável | Onde | Função |
|---|---|---|
| `AI_CLUSTER_JOIN_TOKEN` | Controller, workers, CLI | Segredo de entrada. Obrigatório no controller. |
| `AI_CLUSTER_OPERATOR_TOKEN` | Controller, CLI, gateway, exportador | Segredo de administração. Obrigatório no controller. Precisa ser diferente do de join. |
| `AI_CLUSTER_INFERENCE_TOKEN` | Controller, gateway | Segredo do papel de inferência. Opcional. Diferente dos outros dois. |
| `AI_CLUSTER_API_KEY`, `AI_CLUSTER_TENANTS` | Gateway | Chaves das aplicações. Diferentes de todos os tokens do cluster. |
| `AI_CLUSTER_TLS_CERT`, `AI_CLUSTER_TLS_KEY` | Controller | Certificado do servidor, sempre em par. |
| `AI_CLUSTER_TLS_CA` | Clientes | CA que valida o controller. |
| `AI_CLUSTER_TLS_DOMAIN` | Clientes | Nome verificado quando o endereço é um IP. |
| `AI_CLUSTER_TLS_CLIENT_CA` | Controller | Liga o mTLS: exige certificado de cliente assinado por esta CA. |
| `AI_CLUSTER_TLS_CLIENT_CERT`, `AI_CLUSTER_TLS_CLIENT_KEY` | Workers, CLI, gateway | Certificado de cliente, em par. |
| `AI_CLUSTER_INSECURE` | Controller, clientes, gateway | `1` libera operação sem TLS. Certificado vence esse opt-out. |
| `AI_CLUSTER_AUTO_APPROVE` | Controller | `1` ou `true` aprova todo nó no registro. Padrão: aprovação manual. |
| `AI_CLUSTER_NODES` | Controller | Arquivo com nós, credenciais e aprovação (modo 0600). |
| `AI_CLUSTER_NODE_CREDENTIAL_FILE` | Worker | Arquivo com a credencial do nó. |

Regras comuns:

- Os tokens de join, operador e inferência são distintos entre si. A chave de API do gateway também é distinta dos três. Controller e gateway recusam subir se não forem.
- O servidor compara tokens em tempo constante e checa o token antes de qualquer outra coisa. Quem não se autentica não recebe nenhuma resposta sobre versão ou estado.
- O papel vai marcado na requisição, e cada RPC exige o papel certo.

## Papéis de credencial

| Papel | Credencial | Pode | Não pode |
|---|---|---|---|
| Join | Token de join | Registrar nó (`RegisterNode`). Ler estado: nós, eventos, traces, métricas e reservas. | Aprovar, revogar, enfileirar, reservar, cancelar, ler saídas ou definir release. Não basta para atuar como nó. |
| Operador | Token de operador | Aprovar e revogar nós, enfileirar trabalho e grafos, reservar, liberar reservas, cancelar requisições, ler saídas, definir release, ler tudo que o join lê. Age em nome de qualquer nó. | Atuar como nó: as RPCs de nó exigem a credencial do próprio `node_id`. |
| Inferência | Token de inferência | Só `ShardMatmul`, o cálculo de projeções. Qualquer outra RPC responde `PermissionDenied`. | Aprovar, enfileirar, ler estado ou listar nós. |
| Nó | Credencial do nó | `Heartbeat`, `ReportHealth`, `RequestWork`, `SubmitWorkResult`, `LeaveNode` e o canal de fatias, só para o `node_id` que representa. | Atuar por outro `node_id`. |
| Aplicação | Chave de API do gateway | Rotas `/v1/*` e `/metrics` do gateway. `/health` é público. | Falar com o controller. |

Quem tem o token de join enxerga a lista de nós, os eventos e as métricas, sem poder agir. O token de operador é exigido também pelo exportador de métricas.

O papel de inferência existe para o gateway rodar modelos nos workers sem poder administrar o cluster. O gateway pode usar só esse token; então `/v1/embeddings` (que depende do grafo sintético) fica indisponível.

## Registro e aprovação

1. O nó chama `RegisterNode` com o token de join e recebe uma credencial de 128 bits.
2. Sem `AI_CLUSTER_AUTO_APPROVE`, o nó fica pendente. Um nó pendente não recebe unidades, não entra no placement, não tem a réplica de estado aceita, e `RequestWork` responde sem trabalho (sem erro, para o worker não sair).
3. O operador aprova com `ai-cluster approve <nó>`. Aprovar de novo não tem efeito colateral.
4. Um novo registro com o mesmo `node_id` só mantém a aprovação se provar a credencial atual. Sem prova, o nó volta a pendente.
5. Um nó aprovado sem a credencial é recusado até um `revoke`. Assim, o token de join não expulsa um nó aprovado.

Com `AI_CLUSTER_AUTO_APPROVE`, todo nó é aprovado no registro. Isso é para redes fechadas de teste.

## Transporte

- **TLS de um sentido, por padrão.** O controller serve TLS com o certificado do servidor. O cliente confere a CA em `AI_CLUSTER_TLS_CA`.
- **Sem TLS, o controller não sobe**, a menos que `AI_CLUSTER_INSECURE=1`. Com certificado e `INSECURE` juntos, vale o certificado.
- **mTLS opcional.** Com `AI_CLUSTER_TLS_CLIENT_CA`, o controller exige certificado de cliente assinado pela CA. Peer sem certificado é recusado no handshake, antes de qualquer token. Workers, CLI e gateway apresentam o próprio certificado com `AI_CLUSTER_TLS_CLIENT_CERT` e `AI_CLUSTER_TLS_CLIENT_KEY`.
- **Canal de fatias** com keepalive HTTP/2 de 10 s, para detectar conexão meio-aberta.

Sem `AI_CLUSTER_TLS_CLIENT_CA`, o cliente não apresenta certificado, e a credencial do nó é o único segredo em jogo.

> Planejado: CA do cluster emitindo e renovando certificado por nó, com o controller associando o certificado ao `node_id`.

## Atestação de software

Nas imagens do AI Cluster OS, cada nó mede o próprio slot de boot (SHA-256 da partição ativa) e informa a medida e a versão da imagem. O controller marca um nó como `attested` quando versão e medida são iguais às da release assinada. A coluna `MEDIDA` de `ai-cluster nodes` mostra `confere` nesse caso.

A medida prova que o conteúdo do slot é o esperado. Ela não prova que o nó é íntegro: quem controla o nó controla o que ele informa. Não há TPM, secure boot nem dm-verity nesta versão.

> Planejado: atestação de kernel, driver e versão do worker, com a medida feita por algo fora do controle do sistema operacional, e uso da atestação para liberar trabalho.

## Release assinada

Cada release de imagem é assinada com Ed25519 pelo operador. A chave privada fica fora do repositório e do controller. A assinatura cobre a versão, a placa, o SHA-256 do arquivo comprimido e o SHA-256 da imagem ext4. O URL fica fora da assinatura, então trocar de espelho não exige assinar de novo.

- O nó confere a assinatura antes de baixar qualquer coisa, com as chaves públicas gravadas na imagem.
- Mais de uma chave pública pode estar na imagem, e vale qualquer uma que confira. É isso que permite rotação.
- A versão precisa ser maior que a instalada. Não há downgrade pela via normal.
- O download só vem do host do controller, e só é gravado depois de conferir o SHA-256 e a estrutura gzip.
- Assinatura inválida é recusada, e o nó volta ao cluster. Ver [[ai-cluster-os]] para o fluxo completo.

O controller guarda a release sem verificar a assinatura: a checagem é do nó. Um controller comprometido pode, portanto, tirar nós do cluster por alguns segundos com uma versão inventada, porque o nó drena antes de recusar.

### Rotação de chave

1. Gerar um novo par com `release-keys.sh <nome>`. A privada fica em `~/.aic`.
2. Construir uma imagem de transição com as duas chaves públicas, assinada com a chave antiga.
3. Depois que os nós tiverem a imagem de transição, publicar a imagem seguinte apenas com a chave nova, assinada com a nova.

Não existe lista de revogação. Uma chave vazada vale até a imagem que a retira chegar a cada nó. Quem tem a privada consegue instalar qualquer release em todos os nós.

## Revogação

`ai-cluster revoke <nó>` (operador):

- retira a aprovação e troca a credencial do nó;
- devolve as unidades em voo à fila e tira o nó do mapa de fatias;
- gera o evento `node_revoked` e uma entrada de auditoria;
- faz o worker revogado receber `Unauthenticated`. Se ele subir de novo, nasce pendente.

Revogar também serve para trocar a credencial de um nó. Trocar o token de join ou o de operador exige reiniciar o controller com valores novos.

Limites da revogação:

- Com `AI_CLUSTER_AUTO_APPROVE`, o nó revogado volta aprovado ao se registrar de novo. A revogação só vale com aprovação manual.
- A revogação não envia nada ao worker. Fatias e modelos que ele já guardou continuam no disco dele.
- Não há revogação de certificado, porque não há emissão de certificado por nó.

## Auditoria

Cada ação administrativa gera um evento `audit` com a ação, o alvo e o endereço de quem chamou ([[observabilidade]]). Tentativas negadas não entram na trilha.

Limites:

- O ator é o endereço de rede, não uma identidade. Não há nome de operador nem CN de certificado.
- A trilha é o log de eventos, sem assinatura. Persiste só com `AI_CLUSTER_EVENTS`.
- A ação é registrada antes do efeito.

> Planejado: ator com papel e identificador em cada evento administrativo, e persistência em armazenamento próprio.

## Quarentena

O estado de quarentena existe no modelo, e o placement não escolhe nó em quarentena. Mas nenhum gatilho aplica o estado, e não há comando para entrar ou sair dele. Hoje, a única forma de tirar trabalho de um nó é drená-lo (saída do próprio worker) ou revogá-lo.

> Planejado: quarentena por gatilho de integridade (divergência de medida, comportamento anômalo ou credencial comprometida), que preserva o estado do nó para investigação, com comandos para entrar e sair.

## Segredos

- Os tokens são variáveis de ambiente. Ficam visíveis no ambiente do processo e em ferramentas de inspeção de contêiner.
- A credencial do nó fica em claro no controller (arquivo de `AI_CLUSTER_NODES`, modo 0600) e no worker (arquivo de `AI_CLUSTER_NODE_CREDENTIAL_FILE`).
- A chave de API do gateway é variável de ambiente, lida na subida.
- Não há rotação periódica. Rotação é manual: revogar o nó, ou reiniciar o controller com tokens novos.
- As estruturas de credencial imprimem o segredo se formatadas com `Debug`. Nada no código faz isso hoje.

> Planejado: segredos em arquivo ou cofre, com rotação periódica de credenciais.

## Transporte da perna HTTP

A perna HTTP do gateway não tem TLS. A chave de API viaja em claro fora de loopback, e o gateway só escuta fora de loopback com `AI_CLUSTER_INSECURE=1`. O exportador de métricas também é HTTP puro, com o token de operador como bearer.

> Planejado: TLS na perna HTTP do gateway e no exportador de métricas.

## Autorização por unidade

Hoje o controle é por nó e por papel, não por unidade. Qualquer nó aprovado e com capacidade pode receber qualquer unidade que o placement escolher. Fatias de modelo exigem a credencial do nó. Restaurar estado só vem de nó aprovado.

> Planejado: nó acessa só as unidades, fatias e objetos de estado autorizados a ele.

## Modelo de ameaças

| Ameaça | Controle | Limite |
|---|---|---|
| Cliente anônimo registra nó ou injeta estado | Token de join em toda RPC. Controller não sobe sem ele. | Segredo compartilhado: quem o captura entra. |
| Nó aprova a si mesmo | Aprovação exige o papel de operador. | Quem tem o token de operador aprova qualquer nó. |
| Nó age por outro nó | Credencial por nó em toda RPC de nó. | Credencial em claro sem TLS. |
| Token capturado na rede | TLS por padrão. | Sem TLS com `AI_CLUSTER_INSECURE`; perna HTTP do gateway em claro. |
| Nó comprometido continua recebendo trabalho | Revogação manual. | Sem quarentena automática. |
| Imagem adulterada | Release assinada, conferida pelo nó antes de gravar. | Sem secure boot: quem altera a placa altera o que roda. |
| Versão antiga reinstalada | Versão precisa ser maior. | Sem lista de revogação de chaves. |
| Ação administrativa sem rastro | Evento `audit` no log de eventos. | Ator é endereço de rede; sem assinatura. |

## Limites conhecidos

- **Um segredo de join para todos os nós.** Quem o tiver pode registrar um `node_id` pendente e tentar tomá-lo. Um nó já aprovado só é substituído por revogação.
- **Identidade autodeclarada** até a credencial do nó. O registro pode ser disputado.
- **Operador sem escopo.** O operador age sobre qualquer nó.
- **Segredos em claro** em variáveis de ambiente e em arquivos.
- **Atestação só de software.** Um nó comprometido pode mentir sobre a medida.
- **Sem rotação de chave de release por revogação.** Só por substituição de imagem.
