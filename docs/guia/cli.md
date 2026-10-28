---
title: "CLI ai-cluster"
aliases: ["ai-cluster", "Comandos"]
tags: [guia, cli]
type: guia
---

# CLI `ai-cluster`

A CLI é o painel do operador. Volte ao [[guia]] para o índice; variáveis em [[configuracao]].

```
ai-cluster [--controller <endpoint>] <comando>
```

- `--controller` aponta para o controller. Padrão: `AI_CLUSTER_CONTROLLER`, ou `http://127.0.0.1:50051`.
- Todo comando que fala com o controller usa `AI_CLUSTER_OPERATOR_TOKEN`. Sem ele, o controller recusa. A única exceção é `model`, que trabalha em arquivos locais.
- `-h` e `--help` mostram a ajuda de cada comando.

## Cluster

| Comando | O que faz |
|---|---|
| `status` | Mesmo que `nodes`. |
| `nodes` | Tabela de todos os nós: estado, confiança (`pendente` ou `aprovado`), arquitetura, threads, RAM, scores, saúde, temperatura, swap, classes de armazenamento, versão e medida (`confere` quando a medida bate com a release assinada). |
| `node show <id>` | Tudo que o controller sabe sobre um nó: capacidade, saúde e reserva. |
| `metrics` | Roll-up do cluster: contagem de nós, profundidade da fila e tempos das unidades concluídas. |
| `events [--limit N]` | Eventos recentes, do mais antigo para o mais novo. `--limit` padrão 20; `0` mostra tudo que o controller ainda guarda. |
| `dashboard [--watch N]` | Painel de uma tela: nós, fila e reservas. `--watch N` atualiza a cada N segundos até Ctrl-C. Padrão `0`: imprime uma vez. No terminal, o painel traz o logotipo, seções com régua, estados coloridos e a tendência com seta; redirecionado (scripts), sai em `chave=valor` como sempre. `AI_CLUSTER_COLOR=always` força o painel bonito. |

## Aprovação e saída

| Comando | O que faz |
|---|---|
| `approve <nó> --fingerprint <fp>` | Aprova um nó pendente, para ele receber trabalho. Confere a impressão digital da credencial (`IMPRESSÃO` em `nodes`, também impressa pelo worker ao registrar-se); se o id foi tomado por outro registro, recusa. `--trust-listed` aceita a que o controller mostra agora, sem conferir (anula a proteção). Aprovar de novo não faz mal. Também libera um nó em quarentena. |
| `revoke <nó>` | Retira a aprovação e a credencial do nó, devolve as unidades em voo à fila e registra `node_revoked`. Um worker revogado encerra. |

## Reservas

Uma reserva segura um nó inteiro para uma requisição futura.

| Comando | O que faz |
|---|---|
| `reserve <nó> <request_id> [--ttl-ms N]` | Reserva o nó para a requisição. `--ttl-ms 0` (padrão) usa 60 s. |
| `reservations` | Lista as reservas ativas: nó, requisição dona e vencimento. |
| `release <nó>` | Libera a reserva antes do vencimento. |

## Trabalho

| Comando | O que faz |
|---|---|
| `benchmark [--size N] [--repetitions N] [--local] [--min-threads N] [--request-id ID] [--priority N]` | Despacha unidades de multiplicação de matrizes N×N e compara compute com transferência. `--local` roda o mesmo trabalho no processo, sem despachar. `--min-threads` recusa nós com menos threads. `--request-id` enfileira sob um id existente (por exemplo, o de uma reserva). `--priority` define a classe: maior sai primeiro. Padrões: `--size 256`, `--repetitions 1`, `--min-threads 0`, `--priority 0`. |
| `graph [--size N]` | Enfileira o grafo diamante (A, depois B e C, depois D) e acompanha até drenar. Padrão `--size 128`. |
| `request trace <request_id>` | Unidades de uma requisição, em que estado estão, em que nó rodaram e quanto custaram. |
| `request cancel <request_id>` | Remove da fila as unidades da requisição e manda parar as que já estão em um worker (o aviso vai na próxima batida; a conta é interrompida e o resultado, se vier, é recusado). |

## Modelos (local)

Estes comandos leem e gravam arquivos de um diretório de modelo. Não usam o controller.

| Comando | O que faz |
|---|---|
| `model manifest <dir> --name NOME --version VERSÃO` | Calcula o SHA-256 de cada arquivo e grava `manifest.json` no diretório. |
| `model verify <dir>` | Confere os arquivos do diretório com o `manifest.json`. Sai com erro se não houver manifest ou se algum arquivo diverge. |
| `model list` | Lista os modelos da pasta do controller (`AI_CLUSTER_MODELS_DIR`): formato, arquitetura, parâmetros, precisão, tamanho, contexto, se o motor atual o roda e o motivo quando não. Exige o token de operador. |
| `model check <id> [--ctx N]` | Diz se o cluster comporta o modelo: `OK` ou `NOK` com o motivo (o que falta em GB e quantos nós acrescentar). Soma a RAM dos nós aprovados e vivos (no máximo 80% de cada um, menos 128 MiB) contra pesos, cache de contexto e 10% de margem. Sai com código 1 no NOK. |

## Atualização pelo controller

| Comando | O que faz |
|---|---|
| `rollout <versão> --url URL --sha256 HEX --image-sha256 HEX --signature B64` | Define a release que os nós devem rodar. O canário atualiza primeiro. A assinatura e as somas vêm de `os/image/sign-release.sh`. O controller só confere o formato; a assinatura é conferida no nó. |
| `rollout --cancel` | Encerra o rollout em andamento. |

Atenção: `release <nó>` libera uma reserva. Não tem relação com o rollout, que é `rollout`.
