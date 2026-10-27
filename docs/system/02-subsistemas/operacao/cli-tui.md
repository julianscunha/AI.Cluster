---
title: "CLI e painel"
aliases: ["CLI", "TUI", "ai-cluster", "Painel"]
tags: [sistema/subsistema/operacao]
type: sistema/subsistema
---

# CLI e painel

A operação do cluster é feita pelo shell. A CLI `ai-cluster` é o cliente administrativo: cada comando é uma chamada ao controller, sem estado próprio. O painel é a mesma leitura em tela única, repintada. Nenhum dos dois depende de interface gráfica. Voltar para [[operacao]].

## Configuração

| Variável | Padrão | Uso |
|---|---|---|
| `AI_CLUSTER_CONTROLLER` | `http://127.0.0.1:50051` | Endereço do controller. |
| `AI_CLUSTER_OPERATOR_TOKEN` | — | Token de operador. Todo comando da CLI usa esse token, inclusive os de leitura. |
| `AI_CLUSTER_TLS_CA` | — | CA que valida o certificado do controller. |
| `AI_CLUSTER_TLS_DOMAIN` | — | Nome verificado no certificado, quando o endereço é um IP. |
| `AI_CLUSTER_TLS_CLIENT_CERT`, `AI_CLUSTER_TLS_CLIENT_KEY` | — | Certificado de cliente, quando o controller exige mTLS. |

Ver [[seguranca]] para os papéis de cada token.

## Regras de saída

- Saída que um script lê é `chave=valor` ou tabela de colunas fixas.
- Estado é sempre texto. O painel não usa cor, então funciona em terminal sem cor, com `grep` e em operação remota.
- Comandos que mudam estado respondem com uma linha que diz o resultado, como `NÓ aprovado` ou `NÓ não encontrado`.

## Comandos

### Nós

| Comando | Função |
|---|---|
| `ai-cluster status` | Mesma saída de `nodes`: uma linha por nó, com estado, confiança, versão, medida e sinais. |
| `ai-cluster nodes` | Tabela de nós. |
| `ai-cluster node show <id>` | Um nó, com capability, saúde e confiança completas. |
| `ai-cluster approve <nó>` | Aprova um nó pendente para receber trabalho. |
| `ai-cluster revoke <nó>` | Retira a aprovação e a credencial do nó, devolve o trabalho em voo à fila e deixa o nó pendente. |

A tabela de nós traz as colunas `CONFIANÇA` (`pendente` ou `aprovado`), `VERSÃO` (versão da imagem) e `MEDIDA` (`confere` quando a medida do slot bate com a da release assinada).

### Reservas

| Comando | Função |
|---|---|
| `ai-cluster reserve <nó> <requisição> [--ttl-ms N]` | Retém um nó para uma requisição. `0` usa o padrão do controller (60 s). |
| `ai-cluster reservations` | Lista nó, requisição dona e vencimento. |
| `ai-cluster release <nó>` | Libera a reserva antes do vencimento. Nó sem reserva não é erro. |

Reservar antes de enfileirar permite que a requisição caia no nó reservado: o `benchmark --request-id` aceita o mesmo id da reserva.

### Trabalho e requisições

| Comando | Função |
|---|---|
| `ai-cluster request trace <id>` | Unidades de uma requisição, com estado, nó e tempos. |
| `ai-cluster request cancel <id>` | Remove da fila as unidades de uma requisição. Unidade já em execução termina. |
| `ai-cluster benchmark [--size N] [--repetitions N] [--local] [--min-threads N] [--request-id ID] [--priority N]` | Despacha multiplicações de matrizes e compara compute com transferência. `--local` roda no próprio processo. |
| `ai-cluster graph [--size N]` | Despacha um grafo diamante de dependências e mostra as ramificações liberadas. |

### Métricas e eventos

| Comando | Função |
|---|---|
| `ai-cluster metrics` | Roll-up do cluster em `chave=valor`, com percentis. |
| `ai-cluster events [--limit N]` | Linha do tempo, do mais antigo para o mais recente. Padrão 20. `0` mostra tudo. |
| `ai-cluster dashboard [--watch N]` | Painel de uma tela. `0` ou sem a opção imprime uma vez. |

Ver [[observabilidade]] para o significado de cada campo.

### Release

| Comando | Função |
|---|---|
| `ai-cluster rollout <versão> --url <url> --sha256 <hex> --image-sha256 <hex> --signature <base64>` | Define a release que os nós devem rodar. |
| `ai-cluster rollout --cancel` | Encerra o rollout em andamento. |

O `sign-release.sh` do repositório gera os argumentos de `rollout` a partir do artefato assinado. O fluxo está em [[ai-cluster-os]].

### Modelos (local, sem controller)

| Comando | Função |
|---|---|
| `ai-cluster model manifest <diretório> --name <nome> --version <versão>` | Grava `manifest.json` com nome, versão e SHA-256 de cada arquivo. |
| `ai-cluster model verify <diretório>` | Confere os arquivos contra o manifesto. |

Ver [[model-manager]].

## Painel

`ai-cluster dashboard` mostra, em texto:

- `nós_disponíveis`: quantos nós o placement pode escolher agora.
- Tabela de nós, com estado (`READY`, `DRAINING` e outros) e saúde.
- `TRABALHO`: concluídas, pendentes, bloqueadas e falhas, desde o início do controller.
- `TEMPO`: compute e latência totais e médios, com o previsto ao lado.
- `RESERVAS`: reservas ativas e a requisição dona de cada uma.

Cada atualização relê as chamadas inteiras. Não há streaming, e a tela não tem seleção nem paginação. Detalhe de um nó vem de `node show`, e de uma requisição, de `request trace`.

## Limites conhecidos

- **A CLI usa o token de operador para tudo.** Não existe papel só de leitura para quem só quer olhar o cluster.
- **Painel sem histórico.** Mostra o estado de agora. Taxas por segundo não existem.
- **Painel sem seções de inferência.** TTFT, ITL e tokens por segundo estão no `/metrics` do gateway, não no painel.
- **Sem comandos de manutenção de nó.** A CLI não drena nem reprovisiona um nó. A saída acontece quando o próprio worker se desliga.

> Planejado: `node drain`, `node reprovision` e `node history`.
> Planejado: `request list` e `request show`.
> Planejado: `model list` e `model show`.
> Planejado: `topology`, `diagnose`, `rebalance` e `graph query`.
> Planejado: TUI com navegação e seleção de nó, e estado com cor além do texto.
> Planejado: TTFT, ITL, tokens por segundo e cache KV no painel.
