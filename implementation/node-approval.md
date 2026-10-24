# Aprovação de nó (Fase S2)

O token de join (S1) decide quem pode falar com o controller. Isto decide quem pode
**trabalhar**: um nó que se registra pela primeira vez fica `pendente` até alguém aprová-lo.

## O que existe

- `NodeRecord.trusted`, falso para um nó novo quando o controller exige aprovação. Um nó
  pendente continua vivo: heartbeat e saúde fluem, ele aparece em `ai-cluster nodes`
  (coluna `CONFIANÇA`: `pendente` / `aprovado`) e em `node show` (`confianca=`).
- `registry::eligible` exige `trusted`. Os três consumidores (`is_available`, `candidates`,
  `counts`) passaram a ler o registro inteiro, então um nó pendente nunca ganha unidade, não
  entra no ranking do placement e não conta como disponível.
- `RequestWork` de um nó pendente responde **sem trabalho** (`has_work=false`), não com erro.
  Um erro mataria o worker no primeiro poll, e ele precisa ficar de pé para ser aprovado.
  (Achado pelo E2E: a primeira versão devolvia `FailedPrecondition` e o worker saía.)
- `ai-cluster approve <nó>` chama o RPC `ApproveNode`. Nó desconhecido responde "não
  encontrado"; aprovar duas vezes é inofensivo. Gera o evento `node_approved`.
- Re-registro **mantém** a aprovação só se provar a credencial do nó (S3b, `join-token.md`). Um
  worker reiniciado a perdeu e volta a pendente; o mesmo `node_id` vindo de outro cliente
  também: não herda a confiança de quem foi aprovado.
- A réplica de estado que um nó oferece no registro (I4) é **ignorada enquanto ele está
  pendente**: um nó não aprovado não escreve estado do controller.

## Configuração

Por padrão o controller exige aprovação. `AI_CLUSTER_AUTO_APPROVE=1` (ou `true`) aprova todo nó
no registro, que é o que uma rede de laboratório fechada e os scripts E2E usam. `Registry::new()`
continua confiando em todos (o comportamento anterior); `Registry::requiring_approval()` é o
modo seguro, escolhido em `main`.

## Evidência

`bash code/tests/phase-s2-approval.sh`: A) o nó novo aparece `pendente` e a unidade enfileirada
não é entregue a ele; B) depois de `approve`, a unidade que esperava roda nele e ele consta
`aprovado`; C) `approve` de um nó desconhecido responde "não encontrado". Mais 2 testes
unitários no registry.

## Tetos

- **A aprovação não persiste** além de um reinício do controller: o registry é memória, então
  depois de um restart os nós voltam a pendentes (ou, com a réplica, esperam a aprovação antes
  de poder devolver o estado). Com `AI_CLUSTER_AUTO_APPROVE` isso não aparece.
- **Quem aprova é quem tem o token**: a CLI usa o mesmo segredo dos nós (decisão da S1), então
  um nó comprometido pode aprovar a si mesmo. Separar um token de operador é o próximo degrau.
- Sem revogação: não há comando para desaprovar nem para quarentenar por trust.
- A restrição da réplica pendente não tem teste E2E, só a leitura do código.

## O que a aprovação **não** é

A S2 barra um nó desconhecido, não um nó malicioso. Os achados da revisão sobre o segredo
compartilhado foram fechados depois: autoaprovação pelo token de operador separado (S3a) e
`leave_node` alheio, `submit_work_result` forjado e heartbeat/saúde forjados pela credencial por
nó (S3b). O que resta é o teto de `join-token.md`: tudo em texto claro (sem TLS), sem revogação
nem rotação, e o token de join ainda deixa registrar um `node_id` alheio e derrubá-lo.
