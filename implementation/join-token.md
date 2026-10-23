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

## Tetos

- **Texto claro.** Sem TLS, quem escuta a rede captura o token. E a variável de ambiente
  aparece em `docker inspect`.
- **Segredo compartilhado.** Não há identidade por nó: quem tem o token é qualquer nó.
  Rotação, revogação e credencial por nó (mTLS, docs/19) são os próximos degraus.
- **Fora do escopo:** o exportador de métricas (porta 9101) não é autenticado.
