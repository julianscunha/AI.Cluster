---
title: "F07 — Operador e credencial por nó"
aliases: ["Fase S3a", "Fase S3b", "S3a", "S3b", "operator-token", "credencial-por-no"]
tags: [implementacao/fase/07, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-07-seguranca-base]]"
codigos-antigos: ["S3a", "S3b"]
sistema: ["[[seguranca]]", "[[adr-010-papeis-de-credencial]]"]
evidencia: ["code/crates/protocol/src/metadata.rs", "code/crates/controller/src/service.rs"]
---
# F07 — Operador e credencial por nó

- [ ] Superado

> [!note] Contexto
> O token de join ([[f07-token-de-join]]) identifica "alguém da rede"; ele não manda no cluster.
> Com um segredo só, quem tinha o token aprovava a si mesmo, e um nó podia sair, submeter
> resultado ou forjar heartbeat de outro. Esta fatia separa os papéis: operador (S3a) e credencial
> por nó (S3b).

## Objetivo

Separar quem administra do que é só nó, e dar a cada nó uma credencial própria. Fecha os achados
1, 3 e 4 da revisão da S2.

## O que foi feito

### Token de operador (S3a)

- `AI_CLUSTER_OPERATOR_TOKEN` é obrigatório no controller (fail-closed) e deve ser diferente do de
  join. A CLI o envia no mesmo `AUTH_HEADER`; os workers continuam com o de join.
- `ServerMetadata` aceita os dois e marca a requisição com `Operator` quando o token é o do
  operador. `require_operator` abre as RPCs administrativas: `ApproveNode`, `EnqueueWork`,
  `EnqueueGraph`, `Reserve`, `ReleaseReservation`. Sem a marca: `PermissionDenied`.
- Leituras (`nodes`, `events`, métricas) e as RPCs de nó aceitam qualquer um dos dois.
- `AI_CLUSTER_INFERENCE_TOKEN` (opcional, diferente dos outros dois) é um terceiro papel que só
  chama `ShardMatmul`: ver [[f08-canal-residente-de-fatias]].
- Fecha o achado 1 da revisão de S2 (quem tinha o token aprovava a si mesmo).
- `LeaveNode` ficou aberto nesta fatia, porque o worker o chama ao desligar. Ele é fechado pela
  credencial por nó (S3b).

### Credencial por nó (S3b)

- `RegisterNode` devolve `node_credential` (128 bits aleatórios, `getrandom`). O worker a guarda em
  `NodeCredential` e a envia em `x-ai-cluster-node-auth` em toda chamada seguinte.
- `Heartbeat`, `ReportHealth`, `RequestWork`, `SubmitWorkResult` e `LeaveNode` conferem a
  credencial do `node_id` que dizem representar (`authorize_node`). O operador também passa.
  Um nó desconhecido passa, e o handler dá a resposta de sempre ("não conheço").
- Quem registra um `node_id` existente sem provar a credencial atual recebe uma nova e volta a
  `pendente` (ou `aprovado` com `AI_CLUSTER_AUTO_APPROVE`). Isso substitui a checagem de
  `device_id` da S2, que o cliente podia forjar.
- Com aprovação manual, um nó **aprovado** não é substituído assim: o registro é recusado
  (`FailedPrecondition`) até o operador rodar `ai-cluster revoke` (S5). Assim o token de join não
  expulsa um nó aprovado.
- Fecha os achados 3 e 4 da revisão de S2: `leave_node` alheio e `submit_work_result` forjado.

## Decisões

- **Papéis separados por token, não por identidade.** Dois segredos (join e operador) mais uma
  credencial por nó. Identidade por certificado fica para [[fase-19-hardening-de-producao]].
- **Operador não é nó.** O token de operador não serve como atalho para registrar nó, e o nó não
  consegue chamar RPC administrativa.
- **Credencial é o segredo do nó.** Sem ela, o controller não aceita heartbeat nem resultado do nó.
- Os papéis e a política estão descritos em [[adr-010-papeis-de-credencial]].

## Evidência

- Não há script E2E dedicado a S3a e S3b. A verificação é de código: `require_operator` e
  `authorize_node` (`code/crates/protocol/src/metadata.rs`, `code/crates/controller/src/service.rs`).
- Os scripts `phase-s2-approval.sh` e `phase-s5-persist.sh` usam `approve` e `revoke` pela CLI, que
  envia o token de operador, e os workers que eles sobem se registram com a credencial própria. O
  caminho feliz dos dois papéis passa por eles.

> [!todo] Evidência a criar
> Um E2E de negação: nó com o token de join chamando `ApproveNode` ou `LeaveNode` de outro
> `node_id` deve receber `PermissionDenied`. Hoje só há a leitura do código.

## Limites e próximos passos

- **Preço:** um worker que reinicia perde a credencial. Sem `AI_CLUSTER_NODE_CREDENTIAL_FILE`
  (S5) e sem `AUTO_APPROVE`, precisa ser aprovado de novo.
- **Operador sem rotação própria:** troca-se reiniciando o controller com valor novo. Revogação
  de operador é pendente ([[fase-19-hardening-de-producao]]).
- **Credencial em claro** no controller (memória, e arquivo em S5) e no trânsito, sem TLS até
  [[f07-tls]].
- **Quem tem o token de join** ainda registra um `node_id` alheio **pendente** e tenta tomá-lo. Um
  aprovado só cai com `revoke`.
- **Revogação manual** (`revoke`, S5), sem rotação periódica.
- **Operador age por qualquer nó.** Não há escopo por nó para o operador.
- **Nó desconhecido passa pela checagem.** Um `node_id` que o registro não conhece passa, e o
  handler responde "não conheço". O dono de uma unidade restaurada do disco, ausente do registro,
  pode ter o resultado fechado por quem tem o token de join e o `work_id`.
- **Segredo no `Debug`.** `NodeRecord` e `NodeCredential` imprimem o segredo em `Debug`; nada loga
  hoje, mas é um risco de vazamento a fechar.
- **Níveis intermediários de confiança** (quarentena por trust, do [[saude-e-confianca]]) não
  existem: só pendente/aprovado. Pendente em [[fase-02-capability-e-saude]].

## Relacionados

- Fase: [[fase-07-seguranca-base]] · Sistema: [[seguranca]]
- Irmãos: [[f07-token-de-join]], [[f07-aprovacao-de-no]], [[f07-tls]]
