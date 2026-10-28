# llama.cpp (plano de dados, ADR-022)

O AI Cluster usa o llama.cpp para executar modelos GGUF e o RPC dele para dividir um modelo entre
máquinas. Este diretório guarda só o que o projeto muda e como compilar; o código do llama.cpp vem do
repositório oficial, no commit fixado em `build.sh`.

## `armv7.patch`: três correções para nós de 32 bits

Sem elas, um servidor RPC armv7 (por exemplo o Orange Pi PC) não conversa com um cliente de 64 bits:

1. **`send_rpc_cmd`:** o tamanho da mensagem ia como `size_t` (4 bytes em 32 bits). O protocolo
   documenta 8, e o servidor lia 8, então o enquadramento quebrava no primeiro comando.
2. **`send_msg`:** o mesmo erro, no sentido servidor → cliente.
3. **Alinhamento:** `GGML_MEM_ALIGN` era 4 em 32 bits, mas `ggml_tensor` tem `int64_t` e o clang
   emite cargas NEON com dica de alinhamento de 8 bytes: SIGBUS no Cortex-A7. Passa a 8, com o
   enchimento de `ggml_object` ajustado para continuar múltiplo.

**Limite:** um cliente de 32 bits não usa um servidor de 64 bits (os ponteiros remotos de 64 bits
são truncados no cliente). Por isso o nó principal que carrega o modelo é de 64 bits, e os armv7
entram como servidores RPC.

Essas correções valem ser enviadas ao projeto original.

## Medidas (SmolLM2 360M Q4_K_M)

| Onde roda | Geração |
|---|---|
| PC x86 (4 threads), local | 42 tokens/s |
| Todas as camadas no Pi por RPC (cliente no PC) | 2,3 tokens/s |
| Metade das camadas no Pi por RPC | 3,8 tokens/s |
| Pi local, sem RPC | 2,6 tokens/s |

Dividir um modelo que já cabe no PC com um nó lento **piora** a velocidade (o nó lento limita);
o ganho do RPC aparece quando o modelo não cabe em uma máquina só.
