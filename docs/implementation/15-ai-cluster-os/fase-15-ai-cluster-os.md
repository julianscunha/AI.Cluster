---
title: "F15 — AI Cluster OS e provisionamento"
aliases: ["Fase J — AI Cluster OS (OS)", "Fase 3 — AI Cluster OS", "M2"]
tags: [implementacao/fase/15, implementacao/estado/planejado]
type: implementacao/fase
status: planejado
codigos-antigos: ["EP J1–J6 (OS)", "roadmap 3", "M2", "phases.md Fase 3"]
sistema: ["[[ai-cluster-os]]", "[[provisionamento]]", "[[adr-001-base-linux]]", "[[adr-002-os-imutavel]]"]
---
# F15 — AI Cluster OS e provisionamento

## Objetivo

Fazer uma máquina limpa virar worker READY sem operação manual: imagem imutável com só os serviços necessários, bootstrap a partir de um Linux existente, reimage, boot e recuperação A/B, e suporte a x86_64 antes de ARM64.

> [!note] Colisão de "J"
> O "J" do EP (AI Cluster OS) não é o "J" de inferência (J1–J5 em [[fase-08-inferencia]]). Ver `implementation.md`.

## Depende de

- [[fase-01-nucleo-do-cluster]]: registro e join de nós.
- [[fase-07-seguranca-base]]: credencial por nó e token de join no bootstrap.
- [[fase-10-inteligencia-de-storage]]: layout de disco para a imagem e o reimage.

## Entrega

- Imagem base Linux e serviços mínimos (J1, J2).
- Bootstrap de Linux existente até o enrollment (J3).
- Reimage em instalação limpa (J4).
- Boot e recuperação A/B (J5).
- Multi-arquitetura: x86_64 primeiro, ARM64 em seguida (J6).
- Critério de saída: máquina de teste é reprovisionada automaticamente e volta READY.

## Checklist

Verificado em `code/os/README.md` (só define a fronteira do runtime; o trabalho de imagem começa aqui) e `code/scripts/` (só ambiente de desenvolvimento).

- [ ] J1 Imagem base Linux. Não iniciado.
- [ ] J2 Serviços mínimos. Não iniciado.
- [ ] J3 Bootstrap de Linux existente até o enrollment. Não iniciado.
- [ ] J4 Reimage em instalação limpa. Não iniciado.
- [ ] J5 Boot e recuperação A/B. Não iniciado.
- [ ] J6 Multi-arquitetura. Não iniciado.
- [ ] Critério de saída: reprovisionamento automático e retorno READY. Não atendido.

## Gate

Gate padrão (EP §19): código, benchmarks, telemetria, testes de falha, atualização de documentação e ADR quando a arquitetura mudar.

Gate específico (manifesto): máquina limpa vira worker sem intervenção; M2 completo (bootstrap, imagem instalada automaticamente, nó reingressa READY).

## Slices

Notas desta pasta (esta nota é o MOC): nenhum slice criado.

Slices a criar:

- `f15-imagem-base`: J1.
- `f15-servicos-minimos`: J2.
- `f15-bootstrap-e-enrollment`: J3.
- `f15-reimage`: J4.
- `f15-boot-e-ab`: J5.
- `f15-multi-arch`: J6.

## Códigos antigos

- EP J1–J6 (OS).
- Roadmap 3, itens immutable image, bootstrap, provisioning, reimage, recovery e A/B update.
- Marco M2 (`[ ]`, Fase 3 do roadmap).
- Pacote da Fase 3 em `phases.md`: image build, bootstrap, installer, bootloader, identity persistence, A/B recovery.

## Specs do sistema

- [[ai-cluster-os]]: sistema operacional-alvo.
- [[provisionamento]]: fluxo de provisionamento.
- [[adr-001-base-linux]]: base Linux.
- [[adr-002-os-imutavel]]: imagem imutável.
