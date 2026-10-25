---
title: "F10 — Inteligência de storage"
aliases: ["Fase K — Storage Intelligence", "Fase 4 — Storage/State (parte de storage)", "docs/18", "M3"]
tags: [implementacao/fase/10, implementacao/estado/parcial]
type: implementacao/fase
status: parcial
codigos-antigos: ["EP K1–K5", "EP I2", "roadmap 4 (parte de storage)", "M3"]
sistema: ["[[storage-intelligence]]", "[[state-manager]]", "[[no-e-hardware]]", "[[adr-005-camadas-de-storage]]"]
---
# F10 — Inteligência de storage

## Objetivo

Tratar o armazenamento de cada nó como recurso classificado e medido: detectar as classes (HDD, SSD, NVMe), medir vazão e latência, acompanhar saúde (SMART/NVMe) e alocar espaço por camada (swap, cache de modelo, cache de tensor, checkpoint, reserva). HDD e NVMe devem receber políticas distintas e mensuráveis.

## Depende de

- [[fase-02-capability-e-saude]]: benchmark e capability, onde o score de storage entra.
- [[fase-06-estado-e-recuperacao]]: checkpoint e camadas HOT/WARM/COLD do estado.

## Entrega

- Detecção de classes por nó (K1, feita).
- Benchmark de storage com latência e vazão por classe (K2, parcial).
- Saúde de storage: SMART e NVMe (K3).
- Alocação dinâmica por camada (K4).
- Adaptação de política por desgaste (K5).
- Dashboard com saúde de storage (M3).

## Checklist

Verificado em `code/crates/worker/src/main.rs` (`storage_classes`, storage probe) e `code/crates/cli`.

- [x] K1 Detecção de HDD, SSD e NVMe. Lê `/sys/block/*/queue/rotational` e usa o prefixo `nvme` do nome. SSD SATA e NVMe não se separam só por `rotational`; é heurística, não medida.
- [~] I2 Classes HOT/WARM/COLD por nó. O nó reporta `storage_classes` (ordem: mais quente primeiro). Alocação por camada não existe.
- [~] K2 Benchmark de storage. Mede escrita sequencial com `fsync` (16 MiB, melhor de 3, `storage_score`) e a latência de escrita de 4 KiB com `fsync` (menor de 16, `storage_latency_ms`, `armaz_lat_ms` na CLI). Leitura (o page cache a mascara sem O_DIRECT) e medida por dispositivo não existem.
- [ ] K3 Saúde SMART/NVMe. Não existe.
- [ ] K4 Alocação dinâmica (swap, cache de modelo, cache de tensor, checkpoint, reserva). Não existe; nada coloca estado em disco por camada.
- [ ] K5 Adaptação de política (reduzir escrita quando o desgaste piora). Não existe.
- [ ] Critério de saída: HDD e NVMe recebem políticas distintas e mensuráveis.
- [ ] M3: dashboard mostra saúde do storage. Não existe.

> [!warning] Tetos atuais
> - Um dispositivo gera uma classe: um nó com dois NVMe não se distingue de um com um.
> - Sem leitura; `storage_score` e `storage_latency_ms` são só escrita, do diretório temporário, e não têm consumidor de placement.
> - Em contêiner, `/sys/block` mostra os discos do host, não os do contêiner.

## Gate

Gate padrão (EP §19): código, benchmarks, telemetria, testes de falha, atualização de documentação e ADR quando a arquitetura mudar.

Gate específico (manifesto): políticas HDD × NVMe mensuráveis; M3 completo.

## Slices

Notas desta pasta (esta nota é o MOC):

- [[f10-classes-de-armazenamento]]: classificação por nó (I2), detecção (K1) e benchmark de escrita (K2 parcial). Implementado.

Slices a criar:

- `f10-saude-de-storage`: K3, SMART e NVMe onde houver suporte.
- `f10-alocacao-por-camada`: K4, reserva, swap, cache e checkpoint por classe.
- `f10-politica-por-classe`: K5, política distinta para HDD e NVMe.

## Códigos antigos

- EP K1–K5, I2.
- Roadmap 4, itens "swap/cache policies" e "warm/cold tiers" (`[~]`).
- M3 (`[~]`, Fase 4 do roadmap).
- Pacote de trabalho da Fase 4 em `phases.md` (SSD/HDD/NVMe, SMART, cache dinâmico, swap manager).

## Specs do sistema

- [[storage-intelligence]]: subsistema-alvo de inteligência de storage.
- [[state-manager]]: camadas HOT, WARM e COLD.
- [[no-e-hardware]]: inventário de hardware por nó.
- [[adr-005-camadas-de-storage]]: decisão sobre as camadas.
