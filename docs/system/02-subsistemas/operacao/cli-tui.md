---
title: "CLI e painel"
aliases: ["CLI", "TUI", "aic", "Painel"]
tags: [sistema/subsistema/operacao]
type: sistema/subsistema
---

# CLI e painel

A operação do cluster é feita pelo shell. A CLI `aic` é o cliente administrativo: cada comando `aic cluster …` é uma chamada ao controller, sem estado próprio. O painel é a mesma leitura em tela única, repintada. Nenhum dos dois depende de interface gráfica. A referência completa, com todas as opções e a tabela de migração dos nomes antigos, está em [[cli]]. Voltar para [[operacao]].

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
- Estado é texto. Em terminal o painel usa cor e glifos; redirecionado (scripts, `grep`, log), sai em texto puro, em `chave=valor`.
- Comandos que mudam estado respondem com uma linha que diz o resultado, como `WORKER aprovado` ou `WORKER não encontrado`.

## Comandos

### Workers

| Comando | Função |
|---|---|
| `aic cluster nodes` | Tabela de workers: uma linha por worker, com estado, confiança, versão, medida e sinais. |
| `aic cluster topology` | A estrutura do cluster em árvore: o controller na raiz e os workers agrupados por papel (computação, computação com fatias de modelo, recuperação), com estado, arquitetura, versão e os pendentes de aprovação. O gateway entra como um ramo à parte: ele se apresenta ao controller a cada 15 s (`ReportGateway`, não é um worker e não recebe trabalho) e a árvore mostra o id, a URL, se está vivo e, por modelo, os workers que guardam as fatias. Controller antigo, sem o RPC, mostra a árvore sem o ramo. |
| `aic cluster node show <id>` | Um worker, com capability, saúde e confiança completas. |
| `aic cluster node approve <worker>` | Aprova um worker pendente para receber trabalho. |
| `aic cluster node revoke <worker>` | Retira a aprovação e a credencial do worker, devolve o trabalho em voo à fila e deixa o worker pendente. |
| `aic cluster node retire <worker>` | Tira o worker do trabalho mantendo a identidade: segue conectado, pendente, com a mesma credencial. |
| `aic cluster node remove <worker>` | Esquece o worker por inteiro e guarda a credencial dele como lápide: o processo que ainda a usa é recusado até um convite novo (ADR-027). |

A instalação com aprovação (`aic cluster install list|approve|reject|open|close`) e o convite (`aic cluster invite`) estão na referência de [[cli]].

A tabela de workers traz as colunas `CONFIANÇA` (`pendente` ou `aprovado`), `VERSÃO` (versão da imagem) e `MEDIDA` (`confere` quando a medida do slot bate com a da release assinada).

### Reservas

| Comando | Função |
|---|---|
| `aic cluster reservation add <worker> <requisição> [--minutes N] [--slots N]` | Retém um worker para uma requisição. `--minutes 0` usa o padrão do controller (60 s). `--slots` guarda só N slots; `0` é o worker inteiro. |
| `aic cluster reservation list` | Lista worker, requisição dona e vencimento. |
| `aic cluster reservation remove <worker>` | Libera a reserva antes do vencimento. Worker sem reserva não é erro. |

Reservar antes de enfileirar permite que a requisição caia no worker reservado: o `benchmark --request-id` aceita o mesmo id da reserva.

### Trabalho e requisições

| Comando | Função |
|---|---|
| `aic cluster request trace <id>` | Unidades de uma requisição, com estado, worker e tempos. |
| `aic cluster request cancel <id>` | Remove da fila as unidades de uma requisição. Unidade já em execução termina. |
| `aic cluster benchmark [--size N] [--repetitions N] [--local] [--min-threads N] [--request-id ID] [--priority N]` | Despacha multiplicações de matrizes e compara compute com transferência. `--local` roda no próprio processo. |
| `aic cluster graph [--size N]` | Despacha um grafo diamante de dependências e mostra as ramificações liberadas. |

### Métricas e eventos

| Comando | Função |
|---|---|
| `aic cluster metrics` | Roll-up do cluster em `chave=valor`, com percentis. |
| `aic cluster events [--limit N] [--follow]` | Linha do tempo, do mais antigo para o mais recente. Padrão 20. `0` mostra tudo. Com `--follow`, segue os eventos novos pelo barramento. |
| `aic cluster dashboard [--watch N]` | Painel de uma tela. `0` ou sem a opção imprime uma vez. |

Ver [[observabilidade]] para o significado de cada campo.

### Release

| Comando | Função |
|---|---|
| `aic cluster rollout <versão> --url <url> --sha256 <hex> --image-sha256 <hex> --signature <base64>` (ou `--url <diretório> --linux-signature <base64>` para os workers Linux comum) | Define a release que os workers devem rodar. |
| `aic cluster rollout --cancel` | Encerra o rollout em andamento. |

O `sign-release.sh` do repositório gera os argumentos de `rollout` a partir do artefato assinado. O fluxo está em [[ai-cluster-os]].

### Modelos (local, sem controller)

| Comando | Função |
|---|---|
| `aic cluster model manifest <diretório> --name <nome> --version <versão>` | Grava `manifest.json` com nome, versão e SHA-256 de cada arquivo. |
| `aic cluster model verify <diretório>` | Confere os arquivos contra o manifesto. |

Ver [[model-manager]].

## Painel

`aic cluster dashboard` mostra, em texto:

- `nós_disponíveis`: quantos workers o placement pode escolher agora.
- Tabela de workers, com estado (`READY`, `DRAINING` e outros) e saúde.
- `TRABALHO`: concluídas, pendentes, bloqueadas e falhas, desde o início do controller.
- `TEMPO`: compute e latência totais e médios, com o previsto ao lado.
- `RESERVAS`: reservas ativas e a requisição dona de cada uma.

Cada atualização relê as chamadas inteiras. Não há streaming, e a tela não tem seleção nem paginação. Detalhe de um worker vem de `aic cluster node show`, e de uma requisição, de `aic cluster request trace`.

## Limites conhecidos

- **A CLI usa o token de operador para tudo.** Não existe papel só de leitura para quem só quer olhar o cluster.
- **Painel sem histórico.** Mostra o estado de agora. Taxas por segundo não existem.
- **Painel sem seções de inferência.** TTFT, ITL e tokens por segundo estão no `/metrics` do gateway, não no painel.
- **Sem comandos de manutenção de worker.** A CLI não drena nem reprovisiona um worker. A saída acontece quando o próprio worker se desliga.

> Planejado: `node drain`, `node reprovision` e `node history`.
> Planejado: `request list` e `request show`.
> Planejado: `model show`.
> Planejado: `diagnose`, `rebalance` e `graph query`.
> Planejado: TUI com navegação e seleção de worker, e estado com cor além do texto.
> Planejado: TTFT, ITL, tokens por segundo e cache KV no painel.
