---
title: "CLI ai-cluster"
aliases: ["ai-cluster", "Comandos"]
tags: [guia, cli]
type: guia
---

# CLI `ai-cluster`

A CLI é o painel do operador. Volte ao [[guia]] para o índice; variáveis em [[configuracao]]. A ajuda (`ai-cluster --help`, `<comando> --help`) e os erros de uso saem em português (`crates/cli/src/ptbr.rs` troca os títulos e as mensagens do clap); uma versão em inglês da CLI ainda não existe.

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
| `nodes` | Tabela de todos os nós (a coluna `VELOC` é a velocidade estimada que o plano de modelo usa para ordenar os workers: média geométrica da CPU com a banda de memória; só serve para comparar nós): estado, confiança (`pendente` ou `aprovado`), arquitetura, threads, RAM, scores, saúde, temperatura, swap, classes de armazenamento, versão e medida (`confere` quando a medida bate com a release assinada). |
| `node show <id>` | Tudo que o controller sabe sobre um nó: capacidade, saúde e reserva. |
| `metrics` | Roll-up do cluster: contagem de nós, profundidade da fila e tempos das unidades concluídas. |
| `events [--limit N]` | Eventos recentes, do mais antigo para o mais novo. `--limit` padrão 20; `0` mostra tudo que o controller ainda guarda. |
| `dashboard [--watch N]` | Painel de uma tela: nós, fila e reservas. `--watch N` atualiza a cada N segundos até Ctrl-C. Padrão `0`: imprime uma vez. No terminal, o painel traz o logotipo, seções com régua, estados coloridos e a tendência com seta; redirecionado (scripts), sai em `chave=valor` como sempre. `AI_CLUSTER_COLOR=always` força o painel bonito. |

## Aprovação e saída

| Comando | O que faz |
|---|---|
| `approve <nó> --fingerprint <fp>` | Aprova um nó pendente, para ele receber trabalho. Confere a impressão digital da credencial (`IMPRESSÃO` em `nodes`, também impressa pelo worker ao registrar-se); se o id foi tomado por outro registro, recusa. `--trust-listed` aceita a que o controller mostra agora, sem conferir (anula a proteção). Aprovar de novo não faz mal. Também libera um nó em quarentena. |
| `revoke <nó>` | **Revoga a credencial**: retira a aprovação e a credencial do nó, devolve as unidades em voo à fila e registra `node_revoked`. Um worker revogado encerra. |
| `retire <nó>` | Tira o worker do trabalho **mantendo a identidade**: perde a aprovação (não recebe unidades nem entra no plano de modelo), as unidades em voo voltam à fila, e a credencial e a impressão digital continuam as mesmas. O worker segue conectado, como pendente. Volta com `approve <nó> --fingerprint <a impressão de antes>`. Registra `node_retired`. |
| `remove <nó>` | Remove o worker do cluster: o controller o **esquece por inteiro** (registro e credencial). Se o processo continuar rodando, ele se apresenta de novo como um nó novo e pendente. Registra `node_removed`. |
| `installs` | Aparelhos na etapa de espera de instalação (ADR-023): impressão digital, MAC, IP, CPU, RAM, discos e há quanto tempo esperam. |
| `approve-install <impressão> --disk <disco>` | Aprova a instalação do aparelho, apagando o disco informado (precisa ser um dos que ele listou). Gera o token de instalação, que só o aparelho dono do segredo recebe. |
| `enrollment open [--minutes N]` / `enrollment close` | Abre (1 a 120 min, padrão 15) ou fecha a porta em que os aparelhos se apresentam para instalar. Fechada é o normal; a janela se fecha sozinha. O `aic-setup invite` a abre e fecha por você. |
| `reject-install <impressão>` | Tira o aparelho da lista de espera. |

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
| `model start <id> [--ctx N]` | Sobe o modelo (GGUF) no cluster: confere a RAM, fixa o plano de workers com RPC e cria a chave de API do tenant `default` (mostrada uma vez; o controller guarda o hash). Sai com código 1 no NOK. |
| `model stop` | Tira o modelo do gateway. As chaves continuam valendo. |
| `model status` | O modelo pedido, a geração, os workers do plano, o estado que o gateway reportou (CARREGANDO, PRONTO, FALHOU), os tokens/s do benchmark e os prefixos das chaves, mais os avisos do plano (workers deixados de fora por versão diferente do rpc-server). |
| `model key add <tenant> [--models a,b] [--tokens N] [--class c]` | Cria o tenant e mostra a chave (uma vez). `--models` limita os modelos (padrão: todos), `--tokens` dá a cota por janela e `--class` o teto de prioridade (`background`, `batch`, `normal`, `interactive`, `realtime`). |
| `model key rotate <tenant>` | Troca a chave de um tenant que existe; a antiga deixa de valer na próxima apresentação do gateway (até 5 s) e os limites ficam. |
| `model key remove <tenant>` | Tira o tenant. |
| `model key list` | Os tenants, com o prefixo da chave e os limites (nunca a chave). |

## Atualização pelo controller

| Comando | O que faz |
|---|---|
| `rollout <versão> --url URL --sha256 HEX --image-sha256 HEX --signature B64` | Define a release que os nós devem rodar. O canário atualiza primeiro. A assinatura e as somas vêm de `os/image/sign-release.sh`. O controller só confere o formato; a assinatura é conferida no nó. |
| `rollout --cancel` | Encerra o rollout em andamento. |

Atenção: `release <nó>` libera uma reserva. Não tem relação com o rollout, que é `rollout`.
