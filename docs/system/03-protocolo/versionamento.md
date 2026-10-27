---
title: "Versionamento do protocolo"
aliases: ["protocol/versioning", "Versionamento do protocolo"]
tags: [sistema/protocolo]
type: sistema/protocolo
---

# Versionamento do protocolo

Como o protocolo evolui sem derrubar o cluster, e como o operador atualiza o sistema nos nós. Índice: [[protocolo]]. Mensagens e campos: [[mensagens]].

## Regras

- **Dois eixos de versão.** A versão do protocolo (`x-ai-cluster-protocol-version`) muda quando o significado de uma chamada muda. A versão do sistema vai em `NodeIdentity` (`runtime_version`, `os_version`). Controller e worker reportam os dois separadamente.
- **Schema retrocompatível dentro da janela suportada.** Campo novo recebe número novo. Nunca se reaproveita um número nem se muda o tipo. Campo removido vira `reserved`.
- **Campo novo com efeito semântico** muda o resultado quando ausente, e não é retrocompatível em proto3: o peer antigo usa o padrão sem avisar. Esse tipo de campo exige que controller e workers subam juntos.
- **Nó incompatível é recusado antes de receber carga.** A recusa é por chamada: uma versão diferente de `v1` volta com `FailedPrecondition`, então o nó não registra e não recebe trabalho.

## Estado atual

| Item | Valor |
|---|---|
| Versão do protocolo | `v1` |
| Verificação | o controller rejeita toda chamada com outra versão, inclusive a que não envia versão |
| Negociação de features | não existe; `NodeIdentity` não tem campo de features |
| Quarentena por versão | não existe; a recusa é por chamada |

> [!warning] Atualizar controller e workers juntos
> `by_columns` (`ShardOp` 6, `ShardCompute` 9) e `invalid` (`ShardResult` 8) mudam o resultado quando ausentes. Um worker sem esses campos calcula uma faixa de GPT-2 como se fossem linhas, e pode passar pela checagem de forma do controller sem erro. Antes de deixar workers e controller em versões diferentes, atualize todos juntos. Ver [[canal-de-fatias]].

## Atualização do sistema nos nós (release)

O operador define a versão desejada do sistema com `SetRelease`. Ela tem cinco campos: `version`, `url`, `sha256` (do artefato), `image_sha256` (do conteúdo do slot depois de gravado) e `signature` (Ed25519 em base64, sobre placa, versão, `sha256` e `image_sha256`). O controller recusa a release se:

- a versão tiver caracteres fora de letras, números, `.`, `_`, `-` e `+`;
- algum hash não tiver 64 hexadecimais, ou a assinatura não for base64 de 64 bytes;
- a URL não for `http(s)`, ou tiver usuário, query ou caracteres especiais.

Fluxo:

1. O controller anuncia a release na resposta da batida (`Heartbeat`), só aos nós com `can_update` que rodam outra versão.
2. O primeiro nó elegível vira **canário**. Os demais só recebem a release depois que o canário roda a versão nova e a medida do slot confere com `image_sha256`.
3. Depois do canário, os nós recebem a release em ondas. `AI_CLUSTER_ROLLOUT_WAVE` limita quantos atualizam ao mesmo tempo; zero significa todos.
4. Se o canário não chega à versão em 10 min, o rollout pausa e nada mais é anunciado.
5. Cancelar com `SetRelease` de versão em branco interrompe o rollout.

O nó sai do cluster de forma ordenada (`LeaveNode`), roda o comando de atualização e reinicia. Numa imagem com dois slots, o atualizador confere a assinatura e o SHA-256 antes de gravar o slot inativo. Se o slot novo não sobe, o nó volta ao anterior e não tenta de novo a mesma versão. Quando a release é aceita, o nó mede o slot gravado e informa `image_sha256` no próximo registro, que é como o controller sabe que a atualização terminou.

Rebaixar a versão não é detectado pelo nó, porque não há ordem entre versões. Quem define a release é o operador.

Estratégia: atualize um nó por vez e valide entre cada um. Nunca exija que o cluster inteiro atualize de forma atômica, a menos que a mudança seja uma versão de quebra declarada.

```text
canário -> valida (attested) -> ondas de N nós -> valida -> ...
```

Com o controller ainda em `v1`, um nó novo que fale outra versão é recusado e fica de fora até ser atualizado. Um nó antigo continua funcionando enquanto o protocolo não mudar.

> Planejado: negociação de features no registro, com o controller sabendo o que cada nó suporta antes de mandar uma operação; quarentena de nó incompatível, com estado visível ao operador; política de janela de versões suportadas e de `reserved` no schema; rollback automático de release com validação entre nós.
