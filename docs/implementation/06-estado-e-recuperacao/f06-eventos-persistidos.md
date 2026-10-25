---
title: "F06 — Eventos persistidos"
aliases: ["AI_CLUSTER_EVENTS", "eventos em arquivo"]
tags: [implementacao/fase/06, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-06-estado-e-recuperacao]]"
codigos-antigos: ["H2"]
sistema: ["[[observabilidade]]", "[[state-manager]]"]
evidencia: ["code/tests/phase-i-events.sh"]
---
# F06 — Eventos persistidos

- [ ] Superado

## Objetivo

O `ai-cluster events` era um ring buffer na memória: depois de um restart do controller, justamente quando alguém quer saber o que aconteceu, a linha do tempo estava vazia ([[f05-eventos]]).

## O que foi feito

- `EventLog::persisted(path)` (`AI_CLUSTER_EVENTS` no controller): carrega os últimos 256 eventos do arquivo ao subir e acrescenta cada evento novo, uma linha `ms<TAB>tipo<TAB>detalhe`. Tab e quebra de linha dentro de um campo viram espaço.
- Quando o arquivo chega a 1024 linhas é reescrito só com o que o buffer segura (arquivo temporário e rename), então o tamanho fica limitado sem esquema de rotação.
- Arquivo ilegível ou sem permissão de escrita custa histórico, não o controller. Sem a variável tudo continua só na memória.

## Evidência

`bash code/tests/phase-i-events.sh`: um worker entra (`node_registered`), o controller é removido e sobe de novo com o mesmo arquivo, e `ai-cluster events` ainda mostra o evento. Unitário: `events_survive_a_restart_and_the_file_stays_bounded` (recarga, continuação da linha do tempo, compactação).

## Limites

> [!warning] Histórico curto
> O que o ring buffer já descartou (mais de 256 eventos) não volta; o arquivo existe para atravessar um restart, não para ser um arquivo de auditoria. Cada evento abre o arquivo para acrescentar, sem `fsync`: uma queda de energia pode perder os últimos. Não há assinatura nem proteção contra edição do arquivo.

## Próximos passos

- Exportar eventos a um coletor quando houver um ([[fase-19-hardening-de-producao]]).
