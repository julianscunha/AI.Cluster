---
title: "Segurança e confiança"
aliases: ["docs/19", "19 — Security & Trust", "19 — Segurança e confiança"]
tags: [sistema/subsistema/plataforma]
type: sistema/subsistema
status: proposto
implementacao: parcial
fases: ["[[fase-07-seguranca-base]]", "[[fase-19-hardening-de-producao]]"]
---

# Segurança e confiança

Segurança cobre quem é cada participante do cluster, o que cada um pode fazer, como a
conexão é protegida, como um nó é aceito, suspenso e retirado, e como as ações deixam rastro.
O modelo-alvo separa três identidades (física, lógica e certificado do cluster) e aplica
autorização por papel e por nó. Parte disso já existe no MVP como segredos compartilhados e
credencial por nó; o resto é alvo.

## Princípios

- **Falha fechada.** Controller sem token de join ou de operador não sobe. Token vazio no
  servidor não aceita ninguém. Gateway sem chave de API não sobe.
- **Menor privilégio por papel.** Quem registra e trabalha (nó) não aprova; quem aprova (operador)
  não precisa ser nó; quem só faz inferência (gateway) não toca no plano de controle.
- **Não confiar por padrão.** Nó novo nasce não confiável (`pendente`) e não recebe trabalho até
  ser aprovado.
- **Identidade é do controller.** O que o nó declara sobre si (`node_id`, `device_id`) não é
  prova. A prova é a credencial que o controller emitiu.

## Identidades

| Identidade | Alvo | Hoje |
|---|---|---|
| Física (dispositivo) | Prova de hardware e de imagem de SO, na atestação. | Não existe. `NodeIdentity.device_id` é texto enviado pelo worker, sem verificação. |
| Lógica (nó) | `node_id` autenticado pelo controller, com credencial e certificado. | `node_id` é escolhido pelo worker. A credencial emitida no registro é a prova. |
| Certificado do cluster | Certificado por nó (mTLS) emitido por CA do cluster, com rotação e revogação. | Não existe. O servidor tem certificado; o cliente não apresenta nenhum. |

## Papéis de credencial

A separação em papéis é a decisão de [[adr-010-papeis-de-credencial]].

| Papel | Credencial | Transporte | Pode | Não pode |
|---|---|---|---|---|
| Join | `AI_CLUSTER_JOIN_TOKEN` | `x-ai-cluster-auth` | Registrar nó (`RegisterNode`) e ler estado: `ListNodes`, `ListEvents`, `GetTrace`, `GetMetrics`, `ListReservations`. Chamadas de nó só com a credencial do próprio nó. | Aprovar, revogar, enfileirar, reservar, ler `GetOutput`. Não basta para atuar como nó. |
| Operador | `AI_CLUSTER_OPERATOR_TOKEN` | `x-ai-cluster-auth`; `Bearer` no exportador de métricas | `ApproveNode`, `RevokeNode`, `EnqueueWork`, `EnqueueGraph`, `Reserve`, `ReleaseReservation`, `GetOutput`. Age em nome de qualquer nó (`authorize_node` aceita o operador). Usado pela CLI e pelo scraper. | Ser o nó: as RPCs de nó pedem a credencial do próprio `node_id`, que o operador não substitui. |
| Inferência | `AI_CLUSTER_INFERENCE_TOKEN` (opcional) | `x-ai-cluster-auth` | Só `ShardMatmul`. Qualquer outro caminho responde `PermissionDenied`. Usado pelo gateway. | Aprovar, enfileirar, ler estado. |
| Nó | Credencial por nó, 128 bits (`getrandom`), emitida no `RegisterNode` | `x-ai-cluster-node-auth` | `Heartbeat`, `ReportHealth`, `RequestWork`, `SubmitWorkResult`, `LeaveNode`, `ShardChannel`, somente para o `node_id` que representa. | Atuar por outro `node_id`. |
| Chave de API do gateway | `AI_CLUSTER_API_KEY` | `Authorization: Bearer` nas rotas HTTP | `/v1/models`, `/v1/embeddings`, `/v1/chat/completions`. `/health` é público. | Falar com o controller: é uma credencial de cliente de inferência, separada das quatro acima. |

Regras comuns:

- Os tokens de join, operador e inferência precisam ser distintos entre si; a chave de API do
  gateway também precisa ser distinta dos três. O controller e o gateway recusam subir se não forem.
- O servidor compara com tempo constante (`same_token`) e confere o token antes de qualquer outra
  coisa; quem não se autentica não recebe resposta sobre versão ou estado.
- Papel é marcado na requisição (extensões `Operator` e `Inference`) e cada RPC exige o papel
  certo (`require_operator`, `require_inference`, `authorize_node`).

## Registro e aprovação

Fluxo do nó novo:

1. `RegisterNode` com token de join. O controller devolve a credencial do nó.
2. Sem `AI_CLUSTER_AUTO_APPROVE`, o nó fica `pendente`: não recebe unidade, não entra no
   placement, a réplica de estado que ele oferece é ignorada, e `RequestWork` responde sem trabalho.
3. O operador aprova (`ai-cluster approve`). O nó vira `aprovado`.
4. Re-registro do mesmo `node_id` só mantém a aprovação se provar a credencial atual. Sem prova,
   volta a `pendente`. Nó aprovado sem a credencial é recusado (`FailedPrecondition`) até
   `revoke`, então o token de join não expulsa um nó aprovado.

## Autorização de trabalho

Alvo: um nó só acessa unidades de trabalho, fatias de modelo e objetos de estado autorizados a
ele. Hoje o controle é por nó e por papel, não por unidade: qualquer nó aprovado e com capacidade
pode receber qualquer unidade que o placement escolha. Fatias (`ShardChannel`) exigem a credencial
do nó. Restauração de estado só vem de nó aprovado.

## Transporte e mTLS

Hoje (S4): TLS de um sentido.

- O controller serve TLS com `AI_CLUSTER_TLS_CERT` e `AI_CLUSTER_TLS_KEY` (PEM, sempre em par).
- O cliente (worker, CLI, gateway) confere a CA em `AI_CLUSTER_TLS_CA`. `AI_CLUSTER_TLS_DOMAIN`
  troca o nome verificado quando o endpoint é um IP.
- `http://` só com `AI_CLUSTER_INSECURE=1`, nos dois lados. Certificado vence o opt-out.

mTLS (F19, [[adr-016-mtls-opcional-no-plano-de-controle]]): `AI_CLUSTER_TLS_CLIENT_CA` no controller exige certificado de cliente da CA; worker, CLI e gateway apresentam `AI_CLUSTER_TLS_CLIENT_CERT`/`AI_CLUSTER_TLS_CLIENT_KEY`.

Alvo que falta: cada nó apresenta certificado emitido pela CA do cluster, e o
controller associa o certificado ao `node_id`. A credencial por nó passa a ser complementar, não a
única prova. Os tokens de papel continuam para operador e inferência.

> [!todo] mTLS: o cliente não apresenta certificado; a identidade do nó é só a credencial em
> claro. Falta a CA do cluster, emissão e renovação de certificado por nó.

## Atestação

Alvo: quando disponível, validar imagem de SO, kernel e perfil, versão do worker, estado de driver
e integridade. A atestação pós-boot faz parte do provisionamento: um nó só vira `READY` depois dela
(ver [[provisionamento]]).

Hoje não existe atestação. O único cheque de compatibilidade é a versão do protocolo
(`x-ai-cluster-protocol-version`, `v1`), que diz se os dois lados falam a mesma língua e não diz
nada sobre o que está rodando.

> [!todo] Atestação: nenhum cheque de imagem, kernel, versão ou driver. Depende de imagem de SO
> assinada ([[ai-cluster-os]]) e fica para [[fase-19-hardening-de-producao]].

## Quarentena

Alvo: isolar um nó sem removê-lo. Gatilhos: divergência de integridade, comportamento inesperado,
credencial comprometida. Quarentena tira o nó do placement e preserva o estado para investigação.

Hoje: o enum de estado tem `QUARANTINED` e o placement já o trata como inelegível, mas nenhum
caminho do código aplica esse estado. A única forma de tirar trabalho de um nó é o drain
(`LeaveNode`, pelo próprio worker) ou `revoke`.

> [!todo] Quarentena: o estado existe, mas não há gatilho nem comando. Falta o gatilho de
> integridade e um comando do operador para entrar e sair de quarentena.

## Revogação

Alvo: revogar certificado, invalidar sessões e remover acesso a modelo e estado do nó.

Hoje (S5), `ai-cluster revoke <nó>` (operador):

- retira a aprovação e troca a credencial do nó;
- devolve as unidades em voo à fila e tira o nó do mapa de fatias do controller;
- gera o evento `node_revoked`;
- o worker revogado passa a receber `Unauthenticated` nas chamadas (o script `phase-s5-persist.sh` verifica); se subir de novo, nasce `pendente`.

Revogação também serve de rotação de credencial de um nó. Não há revogação de certificado (não
existe), nem de token de join ou de operador: trocá-los é reiniciar o controller com valores novos.

> [!warning] Com `AI_CLUSTER_AUTO_APPROVE` o nó revogado volta aprovado ao se registrar de novo: a
> revogação só tem efeito com aprovação manual. A revogação não envia nada ao worker: fatias e
> modelo que ele já guardou em cache permanecem no disco dele.

## Segredos

- Os tokens são variáveis de ambiente. Ficam visíveis em `docker inspect` e no ambiente do processo.
- A credencial do nó fica em claro no controller (arquivo de `AI_CLUSTER_NODES`, modo 0600) e no
  worker (arquivo de `AI_CLUSTER_NODE_CREDENTIAL_FILE`).
- Não há rotação periódica. Rotação é manual: revogar o nó, ou reiniciar o controller com novos tokens.
- A chave de API do gateway é uma variável de ambiente, lida na subida.

> [!warning] `Debug` de `NodeRecord` e de `NodeCredential` imprime o segredo. Nada loga isso hoje,
> mas um `{:?}` futuro vaza a credencial.

## Auditoria

Alvo: toda ação administrativa registra quem agiu, quando, em qual nó, com qual papel, e fica
guardada além de uma sessão.

Hoje: o controller guarda os últimos 256 eventos de ciclo de vida em memória
(`node_approved`, `node_revoked`, `node_left`, `node_registered`, `reservation_released` e outros),
e imprime no stdout. O evento não registra quem agiu: `node_approved` guarda só o `node_id`, e o
papel que autorizou a chamada não entra no registro. Nada é persistido.

> [!todo] Auditoria: falta o ator (papel e identificador do chamador) em cada evento administrativo,
> e persistência. Hoje o log é uma visão de incidente recente, não uma trilha.

## Exposição de leitura

Os RPCs de leitura (`ListNodes`, `ListEvents`, `GetTrace`, `GetMetrics`, `ListReservations`) aceitam
o token de join. Quem tem o segredo de join enxerga a lista de nós, os eventos e as métricas, sem
poder agir. `GetOutput` exige o operador.

O exportador de métricas (porta 9101) exige `Authorization: Bearer <token de operador>`, sem TLS:
o token viaja em claro na rede até ser apresentado.

## Modelo de ameaças resumido

| Ameaça | Controle hoje | Lacuna |
|---|---|---|
| Cliente anônimo registra nó ou injeta estado | Token de join obrigatório em toda RPC; controller não sobe sem ele | Token compartilhado; quem o captura entra |
| Nó aprova a si mesmo | Operador separado; aprovação exige papel de operador | Quem tem o token de operador aprova qualquer nó |
| Nó age por outro nó | Credencial por nó em toda RPC de nó | Credencial em claro sem TLS |
| Token capturado na rede | TLS de um sentido, obrigatório salvo `INSECURE` | Sem mTLS; `http` aceito com `INSECURE` |
| Nó comprometido continua recebendo trabalho | Revogação manual | Sem quarentena automática |
| Modelo ou estado em nó revogado | Fatias saem do mapa do controller | A revogação não manda nada ao worker para limpar o cache |
| Ação administrativa sem rastro | Eventos em memória | Sem ator, sem persistência |

> [!info] O que já existe
> - Token de join e falha fechada: [[f07-token-de-join]] (S1).
> - Aprovação de nó e persistência de aprovação: [[f07-aprovacao-de-no]] (S2, S5).
> - Operador, credencial por nó e papel de inferência: [[f07-operador-e-credencial-por-no]] (S3a, S3b).
> - TLS de um sentido e opt-out `AI_CLUSTER_INSECURE`: [[f07-tls]] (S4).
> - Chave de API do gateway e rotas HTTP: [[f08-gateway-e-api]].
> - Token de inferência restrito a `ShardMatmul`: [[f08-canal-residente-de-fatias]] e [[inference-engine]].
> - Eventos de ciclo de vida, base da auditoria atual: [[f05-eventos]].
> - Exportador com Bearer de operador: [[f05-metricas]].
>
> Código: `code/crates/protocol/src/metadata.rs` (papéis, `ServerMetadata`, `InferenceGate`,
> `same_token`, TLS do cliente), `code/crates/controller/src/main.rs` (falha fechada, TLS do servidor,
> `AI_CLUSTER_AUTO_APPROVE`), `code/crates/controller/src/service.rs` (`authorize_node`,
> `require_operator`, `revoke_node`), `code/crates/controller/src/registry.rs` (aprovação,
> credencial, `revoke`), `code/crates/controller/src/exporter.rs` (Bearer), `code/crates/gateway/src/main.rs`
> (`AI_CLUSTER_API_KEY`).

> [!todo] Pendente, na ordem do alvo: mTLS com CA do cluster e certificado por nó; atestação de
> imagem, kernel, worker e driver; quarentena com gatilho e comando; autorização por unidade, fatia e
> objeto de estado; ator e persistência na auditoria; rotação periódica de credenciais; identidade
> física verificada. Ver [[fase-19-hardening-de-producao]].

> [!warning] Desvios do alvo no MVP: um segredo de join para todos os nós (sem identidade própria
> até a credencial de nó); tokens e credenciais em variável de ambiente e em claro; TLS sem mTLS; identidade
> autodeclarada; revogação manual e sem certificado; quarentena sem gatilho; auditoria sem ator.
