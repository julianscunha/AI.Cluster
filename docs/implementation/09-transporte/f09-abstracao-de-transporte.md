---
title: "F09 — Abstração de transporte"
aliases: ["E1", "Transport trait", "Link"]
tags: [implementacao/fase/09, implementacao/fatia, implementacao/estado/parcial]
type: implementacao/fatia
status: parcial
superado: false
fase: "[[fase-09-transporte]]"
codigos-antigos: ["E1", "E3"]
sistema: ["[[camada-de-transporte]]"]
evidencia: ["code/crates/transport/examples/bench.rs"]
---
# F09 — Abstração de transporte

- [ ] Superado

## Objetivo

O trait `Transport` era um esqueleto com `kind()` e nenhuma implementação: todo o tráfego ia por gRPC, direto. Esta fatia dá ao crate `transport` uma interface real (`send`, `receive`, `probe`, e `stream` como repetição dos dois) e dois meios por baixo, para a mesma operação poder ser medida em transportes diferentes — o gate da fase.

## O que foi feito

- `Transport`: `send(&[u8])` envia um payload como um frame; `receive()` espera o próximo payload (`None` é fechamento limpo) e responde pings no caminho; `probe()` mede a ida e volta de um frame vazio.
- `Link<S>` implementa o trait sobre qualquer fluxo ordenado e confiável. Frame: 1 byte de tipo (dado, ping, pong), 4 bytes de tamanho e o payload. Um tamanho acima de `MAX_FRAME_BYTES` (256 MiB) é recusado antes de alocar o buffer.
- Dois meios: `TransportKind::Tcp` (`tcp_connect`/`tcp_accept`, com `TCP_NODELAY`) e `TransportKind::Unix` (`unix_connect`/`unix_accept`, socket de domínio Unix: mesmo host, sem pilha de rede).
- `echo(link)`: o par que devolve cada payload, usado nos testes e na medição.
- `examples/bench.rs`: serializa **o mesmo `WorkUnit`** (protobuf, matrizes `N×N`) e o manda por cada meio, relatando a mediana da ida e volta, a vazão e o ping.

## Evidência

Testes em `crates/transport`: payloads de 0 B a 3 MiB voltam byte a byte e em ordem nos dois meios; o probe é respondido com o servidor em `receive`; frame com tamanho absurdo é recusado; fechamento do par entre frames não é erro.

Medição (`cargo run --release -p ai-cluster-transport --example bench [N]`): a mesma unidade, ida e volta, p50: 64×64 em 142 µs (TCP) e 82 µs (Unix); 256×256 em 650 µs e 706 µs; 512×512 em 3789 µs e 5108 µs. Só as unidades pequenas ganham com o socket Unix; as grandes são limitadas pela cópia. Tabela completa em [[adr-014-abstracao-de-transporte]].

## Limites

> [!warning] O cluster ainda usa gRPC
> Esta camada não substituiu os clientes gRPC do controller e do worker; ela existe ao lado, testada e medida. Mover Work Unit e `ShardChannel` para ela é o passo seguinte e exige decidir o que acontece com autenticação, TLS e metadados, que hoje vêm do gRPC.

> [!todo] Memória compartilhada de verdade (E3)
> O meio `Unix` evita a pilha de rede, mas ainda copia os bytes pelo kernel. Memória compartilhada (anel em `mmap`, sem cópia) é o degrau seguinte, atrás do mesmo trait. `TransportKind::SharedMemory` continua declarado, sem implementação, e `Usb` pertence à [[fase-18-worker-movel]].

## Próximos passos

- Colocar o `ShardChannel` atrás de `Transport` para medir a diferença com carga real de inferência.
- Anel de memória compartilhada como terceiro meio.
