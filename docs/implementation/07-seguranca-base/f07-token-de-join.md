---
title: "F07 — Token de join"
aliases: ["join-token", "Fase S1", "S1"]
tags: [implementacao/fase/07, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-07-seguranca-base]]"
codigos-antigos: ["S1"]
sistema: ["[[seguranca]]"]
evidencia: ["code/tests/phase-s1-join-token.sh"]
---
# F07 — Token de join

- [ ] Superado

> [!note] Contexto
> Até a S1 o controller aceitava qualquer cliente: o `AUTH_HEADER` carregava o literal
> `"anonymous"` e ninguém o conferia. A rede inteira podia registrar um nó, tomar unidades e,
> pior, injetar estado: `RegisterNode` chamava `work.restore(&replica)` com dado do cliente.

## Objetivo

Só quem conhece um segredo de join fala com o controller, em toda RPC, antes de qualquer outra
checagem. É o primeiro degrau de [[seguranca]]; o que vem depois está em [[f07-aprovacao-de-no]],
[[f07-operador-e-credencial-por-no]] e [[f07-tls]].

## O que foi feito

- `AI_CLUSTER_JOIN_TOKEN` é a mesma variável no controller, nos workers e na CLI.
- O controller **recusa subir** sem ela (fail-closed). Um controller sem token aceitaria todos.
- `ServerMetadata` confere o token em **toda** RPC, dos dois serviços (`ClusterControl` e
  `LinkProbe`), antes da checagem de versão: quem não se autentica não aprende mais nada.
  Resposta: `Unauthenticated`. A comparação não tem saída antecipada (`same_token`).
- `ClientMetadata::from_env` lê o token uma vez; sem ele o header vai vazio e o controller recusa.
  Worker e CLI não mudaram de código: ambos já passam por `connect_controller`.
- Um token vazio no servidor não aceita ninguém (e não "todo mundo").

## Decisões

- **Um segredo só nesta fatia.** Join, operador e credencial de nó ficam separados depois (S3a e
  S3b), mas o token de join continua valendo para todo nó que ainda não tem credencial.
- **Fail-closed em tudo:** controller sem token não sobe; servidor com token vazio não aceita.
- **Checagem antes da versão:** um cliente sem token não recebe nem a resposta de incompatibilidade
  de protocolo.

## Evidência

`bash code/tests/phase-s1-join-token.sh`:

- A) controller sem token não sobe.
- B) worker com token errado e worker sem token não aparecem em `nodes`; o log mostra
  `Unauthenticated`.
- C) worker com o token certo fica `READY`.
- D) a CLI sem token é recusada.

Os demais scripts de `code/tests` passam a levar `AI_CLUSTER_JOIN_TOKEN` (padrão `e2e-token`) em
cada `docker run`.

## Limites e próximos passos

- **Texto claro sem TLS** nesta fatia: o token trafega sem cifra. Resolvido com certificado em
  [[f07-tls]].
- **Segredo compartilhado.** Todo nó com o token pode registrar um `node_id` alheio e tomá-lo.
  Parcialmente resolvido pela credencial por nó ([[f07-operador-e-credencial-por-no]]); o
  registro de um `node_id` aprovado continua recusado até a revogação ([[f07-aprovacao-de-no]]).
- **Rotação** do token de join é reiniciar o controller com valor novo; não há rotação periódica
  (pendente, [[fase-19-hardening-de-producao]]).
- **Variável em claro.** Aparece em `docker inspect`; segredo em arquivo ou cofre é pendente de
  [[fase-19-hardening-de-producao]].

## Relacionados

- Fase: [[fase-07-seguranca-base]] · Sistema: [[seguranca]]
- Irmãos: [[f07-aprovacao-de-no]], [[f07-operador-e-credencial-por-no]], [[f07-tls]]
