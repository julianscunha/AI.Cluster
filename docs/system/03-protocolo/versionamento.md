---
title: "Versionamento do protocolo"
aliases: ["protocol/versioning", "Versionamento do protocolo"]
tags: [sistema/protocolo]
type: sistema/protocolo
status: vivo
implementacao: parcial
fases: ["[[fase-00-fundacao]]", "[[fase-19-hardening-de-producao]]"]
---

# Versionamento do protocolo

Como o protocolo evolui sem derrubar o cluster. Índice: [[protocolo]]. Mensagens e campos: [[mensagens]].

## Regras

- **Dois eixos de versão.** A versão do protocolo (`x-ai-cluster-protocol-version`) muda quando o significado de uma chamada muda. A versão do runtime e do sistema vai em `NodeIdentity` (`runtime_version`, `os_version`). Controller e worker reportam os dois separadamente.
- **Schema retrocompatível dentro da janela suportada.** Campo novo recebe número novo; nunca se reaproveita número nem se muda tipo. Campo removido vira `reserved`.
- **Campo novo com efeito semântico** (muda o resultado quando ausente) não é retrocompatível em proto3: o peer antigo usa o padrão sem avisar. Esse tipo de campo exige que controller e worker subam juntos, ou que a mudança seja precedida por um campo de capacidade anunciado no registro.
- **Negociação de features no registro.** Alvo; ainda não existe (ver abaixo).
- **Nó incompatível é recusado antes de receber carga.** Hoje a recusa é por chamada: versão diferente de `v1` volta com `FailedPrecondition`, então o nó não registra nem recebe trabalho.

## Estado atual

| Item | Valor |
|---|---|
| Versão do protocolo | `v1` (`PROTOCOL_VERSION`, `code/crates/protocol/src/metadata.rs`) |
| Verificação | controller rejeita toda chamada com outra versão (inclusive ausente, como `<missing>`) |
| Negociação de features | não existe; `NodeIdentity` não tem campo de features |
| Quarentena por versão | não existe; a recusa é por chamada |

> [!warning] Campos novos com efeito semântico já foram adicionados sem bump
> `by_columns` (`ShardOp` 6, `ShardCompute` 9) e `invalid` (`ShardResult` 8) entraram em `v1`. Um worker anterior a `by_columns` calcula a faixa como linhas de um tensor `[in, out]` e pode passar pela checagem de forma do controller sem erro. Antes de uma release com worker fora de sincronia, o caminho seguro é subir todos os nós juntos, ou elevar a versão do protocolo para `v2` nas chamadas de fatias. Ver [[canal-de-fatias]].

## Estratégia de upgrade

Prefira upgrades graduais: um nó por vez, validando entre cada um.

```text
node01 update
 -> validate
 -> node02
 -> validate
 -> ...
```

Nunca exija que o cluster inteiro atualize de forma atômica, a menos que seja declarado explicitamente como uma release major de quebra.

Com o controller ainda `v1`, um nó novo que falar outra versão é recusado e continua de fora até ser atualizado. Um nó velho continua funcionando enquanto o protocolo não mudar.

> [!info] O que já existe
> - Versão no cabeçalho e rejeição por chamada: [[f01-entrada-e-saida-de-nos]].
> - Metadados de requisição: [[protocolo-controller-worker]].

> [!todo] Falta
> - Negociação de features no registro, com o controller sabendo o que cada nó suporta antes de mandar uma operação.
> - Quarentena de nó incompatível, com estado visível para o operador.
> - Política de janela de versões suportadas e de `reserved` no schema.
> - Upgrade e rollback automatizados com validação entre nós, em [[fase-19-hardening-de-producao]] e [[ai-cluster-os]].
