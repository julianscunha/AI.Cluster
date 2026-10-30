---
title: "ADR-023 — Instalação só com aprovação do controller"
aliases: ["ADR-023", "etapa de espera", "instalação com aprovação"]
tags: [sistema/adr, subsistema/seguranca, subsistema/plataforma]
type: sistema/adr
---

# ADR-023 — Instalação só com aprovação do controller

Status: decidido · registro de pendentes e aprovação implementados (`enrollment.rs`, `AI_CLUSTER_ENROLL_ADDR`); faltam a etapa de espera, o uso do token e o comando gerado · Data: 2026-10-30 · Voltar para [[decisoes]].

## Contexto

O modo dedicado instala o AI Cluster OS por rede ou pendrive ([[adr-017-instalacao-a-b-e-boot-de-rede]]). Hoje o TFTP filtra por IP, e o usuário muitas vezes não sabe o MAC do aparelho novo. O firmware de rede também não envia token: só MAC, UUID e arquitetura. Uma lista de MACs obrigatória não serve, e o gate de entrada já existe depois do boot: o nó nasce pendente e o operador o aprova ([[adr-020-niveis-de-confianca-do-no]]).

## Decisão

1. **Nenhuma instalação começa sozinha.** Qualquer máquina que dá boot (rede, pendrive ou comando em um Linux existente) cai numa **etapa de espera**: imagem mínima, sem a imagem do cluster, sem token e sem CA.
2. **A etapa de espera mostra e informa o resumo:** MAC, CPU, RAM, discos, firmware e uma impressão digital curta. Registra o aparelho no controller como **pendente**, com teto de pendentes (32) e expiração (15 min); o excedente é ignorado.
3. **O operador aprova no controller** conferindo a impressão digital da lista com a da tela. A aprovação mostra qual disco será apagado, e só depois o controller libera a imagem real e um **token de uso único** para aquele nó.
4. **A espera rechecar sozinha** a cada poucos segundos; numa máquina com tela, qualquer tecla força a checagem na hora. Máquina sem monitor não depende de teclado.
5. **Já tem Linux:** o controller gera um comando (como o `/join`) para ingressar no estado atual ou instalar em modo dedicado. No dedicado ele checa o boot (firmware, disco, espaço), mostra o resumo, reinicia e cai na etapa de espera. Não apaga nada antes da aprovação.
6. **O MAC é atalho opcional:** quem já o conhece pode pré-aprová-lo, e o nó entra sem esperar. Não é pré-requisito.

## Consequências

- Uma única entrega cobre "Boot de rede restrito", "Instalador em pendrive" e o token de uso único de "Níveis de confiança".
- A segurança depende do operador conferir a impressão digital: aprovar sem conferir aprova o aparelho de outra pessoa.
- Quem está na mesma rede local pode falsificar o MAC, mas a etapa de espera não entrega nada que sirva a ele.
- Exige um canal de registro anônimo e limitado no controller (pendentes antes de ter credencial); é a parte mais sensível do desenho e precisa de revisão de segurança.

## Implementação (primeira etapa)

- Serviço `Enrollment` (`Announce`) em porta própria e sem interceptor, ligada por `AI_CLUSTER_ENROLL_ADDR`: ligar o anônimo no `ClusterControl` abriria os handlers que dependem do interceptor para exigir o token de join.
- A impressão digital é o início do SHA-256 de um segredo aleatório que o aparelho guarda só na memória. A lista é indexada pelo hash inteiro (a impressão mostrada é só o prefixo; prefixo repetido é recusado). Quem repete o segredo recebe o token até a entrada vencer, para que uma resposta perdida não prenda o aparelho; o "uso único" é do consumo, na etapa futura. Quem copia o MAC não derruba a aprovação de um MAC aprovado nem despeja a entrada de outro endereço.
- A porta anônima tem teto de 64 conexões (a excedente é fechada), vida máxima de 120 s por conexão, timeout de 10 s por chamada e mensagens de até 16 KiB, para não gastar os descritores do plano de controle. A cota por origem vale por IPv4 ou por /64 de IPv6, e endereço desconhecido divide um balde só. Os textos do aparelho entram só em ASCII imprimível.
- Limite conhecido: sem TLS (certificado do controller ainda não fixado na etapa de espera) o segredo e o token trafegam em claro na rede local; quem espiona a rede pode disputar o token com o aparelho. O desenho completo deve fixar a CA do controller na imagem de espera.

## Alternativas

- **Lista de MACs obrigatória:** descartada. O usuário nem sempre sabe o MAC, e o MAC é falsificável.
- **Token na linha de comando do kernel para todos:** descartada. A imagem serviria a qualquer um e o token vazaria para quem baixasse.
- **Entregar a imagem real a quem aparecer e só aprovar o ingresso:** descartada. Quem aparecesse receberia a imagem completa e o disco seria apagado sem confirmação.
