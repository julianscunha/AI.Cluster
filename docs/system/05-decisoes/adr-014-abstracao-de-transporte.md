---
title: "ADR-014 — Abstração de transporte"
aliases: ["ADR-014", "Abstração de transporte"]
tags: [sistema/adr, subsistema/plataforma]
type: sistema/adr
status: decidido
implementacao: parcial
fases: ["[[fase-09-transporte]]"]
---

# ADR-014 — Abstração de transporte

Data: 2026-10-25 · Voltar para [[decisoes]].

## Contexto

Todo o tráfego do cluster vai por gRPC sobre TCP, inclusive entre processos do mesmo host. O crate `transport` tinha um trait sem métodos úteis e nenhum meio por baixo. A fase pede que a mesma operação seja carregada por transportes distintos, com o custo de cada um medido, antes de qualquer decisão de trocar o transporte do cluster.

## Decisão

1. **O transporte carrega bytes, não operações.** O trait `Transport` envia e recebe payloads já serializados (`send`, `receive`, `probe`); quem serializa um Work Unit ou uma fatia é a operação. Assim a operação não muda quando o meio muda.
2. **Frames, não fluxo cru.** Cada payload vai num frame de tipo + tamanho (u32) + bytes. O tamanho é limitado (`MAX_FRAME_BYTES`) e checado antes de alocar. Ping e pong dividem o canal com os dados, o que permite medir latência sem abrir outra conexão.
3. **Um `Link<S>` genérico sobre fluxos tokio.** Cada meio novo é só um construtor. Hoje: TCP e socket de domínio Unix. Memória compartilhada (anel em `mmap`) entra do mesmo jeito, sem mudar o trait.
4. **Async nativo no trait, sem `dyn`.** O código escolhe o meio onde abre o enlace e usa o tipo concreto; não há objeto de transporte em tempo de execução. Evita a dependência de `async-trait` e a alocação por chamada.
5. **O cluster continua em gRPC por enquanto.** A camada existe ao lado, testada e medida. Trocar o caminho de Work Unit ou o `ShardChannel` para ela é uma decisão própria, porque o gRPC hoje também entrega TLS, autenticação por metadados e propagação de erro.

## Medição

Medição de `examples/bench.rs` (contêiner de desenvolvimento, mesmo host, `--release`, 40 rodadas, ida e volta de um `WorkUnit` pelo par de eco):

| Unidade (matrizes N×N) | Payload | TCP p50 | Unix p50 | Ping TCP / Unix |
|---|---|---|---|---|
| 64 | 0,06 MiB | 142 µs (925 MB/s) | 82 µs (1609 MB/s) | 70 / 71 µs |
| 256 | 1,00 MiB | 650 µs (3229 MB/s) | 706 µs (2971 MB/s) | 166 / 76 µs |
| 512 | 4,00 MiB | 3789 µs (2214 MB/s) | 5108 µs (1642 MB/s) | 108 / 142 µs |

O socket Unix ganha só nas unidades pequenas (menos custo fixo por frame). A partir de ~1 MiB a vazão é limitada pela cópia no kernel e pelo `Vec` do receptor, e o TCP em loopback empata ou vence. Ou seja: sair da pilha de rede **não** é o ganho que justifica a memória compartilhada; o que justifica é evitar a cópia, e isso este meio não faz. Números de um host só e de uma execução, indicativos.

## Consequências

- A medição da fase é reproduzível: `cargo run --release -p ai-cluster-transport --example bench [N]` manda o mesmo `WorkUnit` por cada meio.
- Dois meios no mesmo host respondem a pergunta "vale a pena sair da pilha de rede?" antes de investir em memória compartilhada.
- Quem usar o crate precisa tratar autenticação por conta própria (no socket Unix, pelas permissões do arquivo; no TCP, por TLS por cima), porque o frame não carrega credencial.

## Alternativas

- Estender o gRPC com um canal em memória (tonic sobre `UnixStream`): reaproveita todo o resto, mas ata a escolha de meio ao tonic e não cobre USB nem memória compartilhada.
- Trait de operação (`send_work_unit`, `send_slice`): acopla o transporte ao catálogo de mensagens e multiplica a superfície a cada operação nova.
- Objeto `dyn Transport`: permitiria trocar o meio em execução, mas custa uma alocação por chamada e nada hoje exige a troca.

> [!warning] Tetos
> Sem memória compartilhada, sem USB, sem compressão, sem multiplexação de várias operações no mesmo enlace e sem reconexão. A autenticação e o TLS não existem na camada. Perda de pacotes não é medida: sobre meios confiáveis ela aparece como atraso.

> [!info] O que já existe
> `Transport`, `Link`, TCP e Unix, `echo`, `probe` e o benchmark, em `code/crates/transport`. Medição de latência de enlace do worker ao controller em [[f09-metricas-de-enlace]]. Slices: [[f09-abstracao-de-transporte]].

> [!todo] Memória compartilhada (E3), integração com o `ShardChannel` e o caminho de Work Unit, e USB ([[fase-18-worker-movel]]).
