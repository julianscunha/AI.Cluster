---
title: "ADR-014 — Abstração de transporte"
aliases: ["ADR-014", "Abstração de transporte"]
tags: [sistema/adr, subsistema/plataforma]
type: sistema/adr
---

# ADR-014 — Abstração de transporte

Status: decidido · Data: 2026-10-25 · Voltar para [[decisoes]].

## Contexto

Todo o tráfego do cluster vai por gRPC sobre TCP, inclusive entre processos do mesmo host. A camada de transporte precisa poder carregar a mesma operação por meios diferentes, com o custo de cada um medido, antes de qualquer decisão de trocar o transporte do cluster.

## Decisão

1. **O transporte carrega bytes, não operações.** O trait `Transport` envia e recebe payloads já serializados (`send`, `receive`, `probe`); quem serializa um Work Unit ou uma fatia é a operação. Assim a operação não muda quando o meio muda.
2. **Frames, não fluxo cru.** Cada payload vai num frame de tipo + tamanho (u32) + bytes. O tamanho é limitado (`MAX_FRAME_BYTES`) e checado antes de alocar. Ping e pong dividem o canal com os dados, o que permite medir latência sem abrir outra conexão.
3. **Um `Link<S>` genérico sobre fluxos tokio.** Cada meio novo é só um construtor. Hoje: TCP e socket de domínio Unix. Memória compartilhada (anel em `mmap`) entra do mesmo jeito, sem mudar o trait.
4. **Async nativo no trait, sem `dyn`.** O código escolhe o meio onde abre o enlace e usa o tipo concreto; não há objeto de transporte em tempo de execução. Evita a dependência de `async-trait` e a alocação por chamada.
5. **O cluster continua em gRPC por enquanto.** A camada existe ao lado, testada e medida. Trocar o caminho de Work Unit ou o `ShardChannel` para ela é uma decisão própria, porque o gRPC hoje também entrega TLS, autenticação por metadados e propagação de erro.

## Consequências

- Em um mesmo host, o socket Unix só ganha em unidades pequenas (menos custo fixo por frame). A partir de cerca de 1 MiB, o TCP em loopback empata ou vence, porque a vazão é limitada pela cópia no kernel e pelo buffer do receptor. Sair da pilha de rede não justifica sozinho a memória compartilhada; o que a justifica é evitar a cópia, e este meio não faz isso.
- Dois meios no mesmo host respondem a pergunta "vale a pena sair da pilha de rede?" antes de investir em memória compartilhada.
- A operação `examples/bench.rs` do crate `transport` manda o mesmo `WorkUnit` por cada meio e mede a ida e volta.
- Quem usar o crate precisa tratar autenticação por conta própria (no socket Unix, pelas permissões do arquivo; no TCP, por TLS por cima), porque o frame não carrega credencial.

## Alternativas

- Estender o gRPC com um canal em memória (tonic sobre `UnixStream`): reaproveita todo o resto, mas ata a escolha de meio ao tonic e não cobre USB nem memória compartilhada.
- Trait de operação (`send_work_unit`, `send_slice`): acopla o transporte ao catálogo de mensagens e multiplica a superfície a cada operação nova.
- Objeto `dyn Transport`: permitiria trocar o meio em execução, mas custa uma alocação por chamada e nada hoje exige a troca.

## Limites

- Sem memória compartilhada, sem USB, sem compressão, sem multiplexação de várias operações no mesmo enlace e sem reconexão.
- A autenticação e o TLS não existem na camada.
- Perda de pacotes não é medida: sobre meios confiáveis ela aparece como atraso.
- Planejado: memória compartilhada, integração com o `ShardChannel` e com o caminho de Work Unit, e USB ([[camada-de-transporte]]).
