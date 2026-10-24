# Token de join (Fase S1)

Até aqui o controller aceitava qualquer cliente: o `AUTH_HEADER` carregava o literal
`"anonymous"` e ninguém o conferia. Isso deixava a rede inteira registrar um nó, tomar unidades
e, pior, injetar estado: `RegisterNode` chama `work.restore(&replica)` com dado do cliente.

## O que existe

- `AI_CLUSTER_JOIN_TOKEN` é a mesma variável no controller, nos workers e na CLI.
- O controller **recusa subir** sem ela (fail-closed). Um controller sem token aceitaria todos.
- `ServerMetadata` confere o token em **toda** RPC, dos dois serviços (`ClusterControl` e
  `LinkProbe`), antes da checagem de versão: quem não se autentica não aprende mais nada.
  Resposta: `Unauthenticated`. A comparação não tem saída antecipada (`same_token`).
- `ClientMetadata::from_env` lê o token uma vez; sem ele o header vai vazio e o controller
  recusa. Worker e CLI não mudaram de código: ambos já passam por `connect_controller`.
- Um token vazio no servidor não aceita ninguém (e não "todo mundo").

## Evidência

`bash code/tests/phase-s1-join-token.sh`: A) controller sem token não sobe; B) worker com
token errado e worker sem token não aparecem em `nodes` e o log mostra `Unauthenticated`;
C) worker com o token certo fica READY; D) a CLI sem token é recusada. Os demais scripts de
`code/tests` passam a levar `AI_CLUSTER_JOIN_TOKEN` (padrão `e2e-token`) em cada `docker run`.

## Token de operador (Fase S3a)

O token de join identifica "alguém da rede"; ele não manda no cluster.

- `AI_CLUSTER_OPERATOR_TOKEN` é obrigatório no controller (fail-closed) e deve ser diferente do
  de join. A CLI o envia no mesmo `AUTH_HEADER`; os workers continuam com o de join.
- `ServerMetadata` aceita os dois e marca a requisição com `Operator` quando o token é o do
  operador. `require_operator` abre as RPCs administrativas: `ApproveNode`, `EnqueueWork`,
  `EnqueueGraph`, `Reserve`, `ReleaseReservation`. Sem a marca: `PermissionDenied`.
- Leituras (`nodes`, `events`, métricas) e as RPCs de nó aceitam qualquer um dos dois.
- Fecha o achado 1 da revisão de S2 (quem tem o token aprovava a si mesmo). `LeaveNode` fica
  aberto, porque o worker o chama ao desligar; ele depende da credencial por nó (S3b).
- Teto: continua em texto claro, sem TLS; o operador não tem rotação nem revogação.

## Credencial por nó (Fase S3b)

O token de join diz "alguém da rede"; a credencial diz "este nó".

- `RegisterNode` devolve `node_credential` (128 bits aleatórios, `getrandom`). O worker a guarda
  em `NodeCredential` e a manda em `x-ai-cluster-node-auth` em toda chamada seguinte.
- `Heartbeat`, `ReportHealth`, `RequestWork`, `SubmitWorkResult` e `LeaveNode` conferem a
  credencial do `node_id` que dizem representar (`authorize_node`); o operador também passa.
  Um nó desconhecido passa, e o handler dá a resposta de sempre ("não conheço").
- Quem registra um `node_id` existente sem provar a credencial atual recebe uma nova e volta a
  `pendente` (ou `aprovado` com `AI_CLUSTER_AUTO_APPROVE`). Isso substitui a checagem de
  `device_id` da S2, que o cliente podia forjar.
- Fecha os achados 3 e 4 da revisão de S2: `leave_node` alheio e `submit_work_result` forjado.

**Preço:** um worker que reinicia perdeu a credencial (só vive em memória) e, sem
`AUTO_APPROVE`, precisa ser aprovado de novo. **Tetos:** a credencial é guardada em claro no
controller e trafega em claro, sem TLS; quem tem o token de join ainda pode registrar um
`node_id` alheio e derrubá-lo (negação de serviço, não falsificação); não há revogação nem
rotação periódica. O operador age por qualquer nó. Um `node_id` que o registro não conhece passa pela
checagem; o dono de uma unidade restaurada do disco, ausente do registro, pode ter o resultado
fechado por quem tem o token de join e o `work_id` (o `Debug` de `NodeRecord` e de
`NodeCredential` também imprime o segredo, mas nada o loga hoje).

## Tetos

- **Texto claro.** Sem TLS, quem escuta a rede captura o token. E a variável de ambiente
  aparece em `docker inspect`.
- **Segredo compartilhado.** Não há identidade por nó: quem tem o token é qualquer nó.
  Rotação, revogação e credencial por nó (mTLS, docs/19) são os próximos degraus.
- **Fora do escopo:** o exportador de métricas (porta 9101) não é autenticado.
