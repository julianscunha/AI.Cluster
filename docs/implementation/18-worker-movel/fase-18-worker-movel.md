---
title: "F18 — Worker móvel"
aliases: ["Fase Q — Mobile Worker", "E4 (USB)", "M7"]
tags: [implementacao/fase/18, implementacao/estado/planejado]
type: implementacao/fase
status: planejado
codigos-antigos: ["EP Q1–Q6", "EP E4 (USB)", "roadmap 7 (USB transport, mobile worker prototype)", "M7"]
sistema: ["[[worker-runtime]]", "[[no-e-hardware]]", "[[camada-de-transporte]]", "[[saude-e-confianca]]", "[[ai-cluster-os]]", "[[hotplug-e-recuperacao]]"]
---
# F18 — Worker móvel

## Objetivo

Aceitar celulares e similares como workers: matriz de hardware suportada, reimage pelo caminho de bootloader, imagem ARM64, transporte USB, backend de aceleração móvel e limites térmicos e de bateria. Um nó móvel só entra se o valor computacional líquido, depois de comunicação e limites térmicos, for positivo.

## Depende de

- [[fase-09-transporte]]: transporte USB (E4 movido para cá).
- [[fase-15-ai-cluster-os]]: reimage e boot.
- [[fase-16-aceleradores]]: backend móvel e ARM64.

## Entrega

- Matriz de hardware suportada (Q1).
- Caminho de bootloader e reimage (Q2).
- Imagem ARM64 (Q3).
- Transporte USB (Q4).
- Backend de acelerador móvel (Q5).
- Limites térmicos e de bateria (Q6).

## Checklist

Verificado em `code/crates/worker/src/main.rs` (score térmico em `health`) e `code/crates/transport` (`TransportKind::Usb` declarado, sem implementação).

- [ ] Q1 Matriz de hardware suportada. Não existe.
- [ ] Q2 Bootloader e reimage. Não existe.
- [ ] Q3 Imagem ARM64. Não existe.
- [ ] Q4 Transporte USB. Não existe (variante do enum sem implementação).
- [ ] Q5 Backend de acelerador móvel. Não existe.
- [~] Q6 Limites térmicos e de bateria. Térmico entra no score de saúde (pressão térmica, C4). Bateria não é medida.
- [ ] Critério de saída: celular compatível reimaged, conectado por USB e contribuindo com compute mensurável sem comprometer o cluster (M7).

## Gate

Gate padrão (EP §19): código, benchmarks, telemetria, testes de falha, atualização de documentação e ADR quando a arquitetura mudar.

Gate específico (manifesto): M7 completo. Condição do EP: o compute líquido do nó móvel, depois de comunicação e limites térmicos, precisa ser positivo; caso contrário, o nó não entra no cluster.

## Slices

Notas desta pasta (esta nota é o MOC): nenhum slice criado.

Slices a criar:

- `f18-matriz-de-hardware`: Q1.
- `f18-bootloader-e-reimage`: Q2.
- `f18-imagem-arm64`: Q3.
- `f18-transporte-usb`: Q4 e E4.
- `f18-backend-movel`: Q5.
- `f18-limites-termicos-e-bateria`: Q6.

## Códigos antigos

- EP Q1–Q6.
- EP E4 (USB), movido de F09.
- Roadmap 7 (USB transport, mobile worker prototype).
- Marco M7 (`[ ]`, Fase 7 do roadmap).

## Specs do sistema

- [[worker-runtime]]: runtime do worker móvel.
- [[no-e-hardware]]: inventário e limites de hardware.
- [[camada-de-transporte]]: transporte USB.
- [[saude-e-confianca]]: score térmico e de bateria.
- [[ai-cluster-os]]: imagem e reimage.
- [[hotplug-e-recuperacao]]: conexão e desconexão de USB.
