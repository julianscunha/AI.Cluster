---
title: "F07 — Aprovação de nó"
aliases: ["node-approval", "Fase S2", "Fase S5", "S2", "S5"]
tags: [implementacao/fase/07, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-07-seguranca-base]]"
codigos-antigos: ["S2", "S5"]
sistema: ["[[seguranca]]", "[[adr-010-papeis-de-credencial]]"]
evidencia: ["code/tests/phase-s2-approval.sh", "code/tests/phase-s5-persist.sh"]
---
# F07 — Aprovação de nó

- [ ] Superado

> [!note] Contexto
> O token de join ([[f07-token-de-join]]) decide quem pode falar com o controller. Esta fatia decide
> quem pode **trabalhar**: um nó que se registra pela primeira vez fica `pendente` até alguém
> aprová-lo. A S5 acrescenta persistência da aprovação e revogação.

## Objetivo

Nó novo não recebe trabalho nem escreve estado até aprovação do operador. Nó aprovado pode ser
revogado, e a aprovação sobrevive a restart quando o operador pede.

## O que foi feito

### Aprovação (S2)

- `NodeRecord.trusted`, falso para nó novo quando o controller exige aprovação. Um nó pendente
  continua vivo: heartbeat e saúde fluem, e ele aparece em `ai-cluster nodes` (coluna `CONFIANÇA`:
  `pendente` / `aprovado`) e em `node show` (`confianca=`).
- `registry::eligible` exige `trusted`. Os três consumidores (`is_available`, `candidates`,
  `counts`) leem o registro inteiro, então um nó pendente nunca ganha unidade, não entra no ranking
  do placement e não conta como disponível.
- `RequestWork` de um nó pendente responde **sem trabalho** (`has_work=false`), não com erro. Um
  erro mataria o worker no primeiro poll, e ele precisa ficar de pé para ser aprovado. (Achado do
  E2E: a primeira versão devolvia `FailedPrecondition` e o worker saía.)
- `ai-cluster approve <nó>` chama o RPC `ApproveNode`. Nó desconhecido responde "não encontrado";
  aprovar duas vezes é inofensivo. Gera o evento `node_approved`.
- A réplica de estado que um nó oferece no registro (F06, I4) é **ignorada enquanto ele está
  pendente**: um nó não aprovado não escreve estado do controller.

### Persistência e revogação (S5)

- `AI_CLUSTER_NODES=<arquivo>` no controller guarda `node_id`, credencial e aprovação (modo 0600,
  em claro). Sem a variável, a memória vale só enquanto o controller vive. Um nó que **sai**
  (`leave_node`) deixa de constar em `nodes`, mas o controller lembra dele.
- `AI_CLUSTER_NODE_CREDENTIAL_FILE=<arquivo>` no worker guarda a credencial recebida. Ao reiniciar,
  o worker a apresenta no registro e **mantém a aprovação**; sem o arquivo, volta a pendente. Com o
  arquivo do controller perdido, todos voltam a pendente.
- `ai-cluster revoke <nó>` (operador) retira a aprovação, troca a credencial, devolve as unidades
  em voo à fila e gera o evento `node_revoked`. O worker revogado encerra com `Unauthenticated`; se
  subir de novo, nasce pendente. Revogar também esquece um nó que já saiu.
- Com aprovação manual, registrar o id de um nó aprovado sem a credencial dele é recusado até o
  `revoke`: é o caminho para um worker que perdeu o arquivo de credencial.
- `revoke` é também a **rotação** de credencial de um nó. Rotação dos tokens de join e de operador
  é reiniciar o controller com valores novos; a de certificado, trocar os PEM e reiniciar.

### Configuração

Por padrão o controller exige aprovação. `AI_CLUSTER_AUTO_APPROVE=1` (ou `true`) aprova todo nó no
registro, que é o que uma rede de laboratório fechada e os scripts E2E usam. `Registry::new()`
continua confiando em todos (comportamento anterior); `Registry::requiring_approval()` é o modo
seguro, escolhido em `main`.

## Decisões

- **Pendente por padrão.** Nó desconhecido não recebe trabalho; a exceção é explícita
  (`AUTO_APPROVE`).
- **Pendente não é erro.** `RequestWork` responde sem trabalho para não derrubar o worker.
- **Confiança não é herdada.** Um `node_id` vindo de outro cliente, ou de outro dispositivo, não
  herda aprovação; quem prova a credencial atual mantém.
- **Revogação é do operador.** Só o token de operador retira aprovação ([[f07-operador-e-credencial-por-no]]).

## Evidência

- `bash code/tests/phase-s2-approval.sh`:
  - A) o nó novo aparece `pendente` e a unidade enfileirada não é entregue a ele.
  - B) depois de `approve`, a unidade que esperava roda nele e ele consta `aprovado`.
  - C) `approve` de um nó desconhecido responde "não encontrado".
  - Mais 2 testes unitários no registry.
- `bash code/tests/phase-s5-persist.sh`: A/B) a aprovação sobrevive ao reinício do worker e ao do
  controller (`AI_CLUSTER_NODES`); C) um impostor com o mesmo `node_id` é recusado e o aviso manda
  revogar; D) `revoke` devolve o nó a pendente, o worker revogado encerra com `Unauthenticated`, e
  revogar um nó desconhecido responde "não encontrado".

> [!todo] Evidência a completar
> A restrição da réplica pendente (um nó pendente não escreve estado) não tem teste E2E; só a
> leitura do código.

## Limites e próximos passos

- **Aprovação sem persistência por padrão.** Sem `AI_CLUSTER_NODES`, o registry é memória: depois de
  restart os nós voltam a pendentes (ou, com a réplica, esperam aprovação antes de devolver o
  estado). Com `AI_CLUSTER_AUTO_APPROVE` isso não aparece.
- **Revogação sem efeito com `AUTO_APPROVE`.** O nó revogado volta aprovado ao se registrar de novo,
  então a revogação só tem efeito com aprovação manual.
- **Worker não sobrevive a controller fora do ar.** Um orquestrador precisa reergue-lo.
- **Segredos em claro** nos dois arquivos (`AI_CLUSTER_NODES` e de credencial). Pendente de
  [[fase-19-hardening-de-producao]].
- **Quarentena por trust** (sem os níveis intermediários de [[saude-e-confianca]]) não existe:
  só pendente e aprovado.
- **O que a aprovação não é:** a S2 barra nó desconhecido, não nó malicioso. O que resta contra
  nó malicioso (token de join que registra `node_id` alheio pendente) está em
  [[f07-operador-e-credencial-por-no]] e [[f07-token-de-join]].

## Relacionados

- Fase: [[fase-07-seguranca-base]] · Sistema: [[seguranca]]
- Irmãos: [[f07-token-de-join]], [[f07-operador-e-credencial-por-no]], [[f07-tls]]
