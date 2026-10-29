---
title: "State Manager"
aliases: ["State Manager", "Gerenciador de estado"]
tags: [sistema/subsistema/dados]
type: sistema/subsistema
---

# State Manager

O state manager guarda o estado necessário para retomar a execução quando um nó ou o controller falha. Ele não executa nada: guarda, replica e restaura. Voltar para [[dados]].

## O que é guardado hoje

Só o estado de trabalho do controller:

| Conteúdo | Onde | Quando é gravado |
|---|---|---|
| Fila de Work Units pendentes | Arquivo do controller (`AI_CLUSTER_STATE`) | A cada alteração da fila, em voo ou concluída. |
| Unidades em voo, com o dono de cada uma | Mesmo arquivo | Junto da alteração que a levou a voo. |
| Resultados que uma unidade pendente ainda lê | Mesmo arquivo | Enquanto houver leitor. |
| Cópia do arquivo | Arquivo ao lado (`.replica`), no mesmo volume | Junto do arquivo principal. |
| Cópia de nó | Arquivo em cada worker com `AI_CLUSTER_STATE_REPLICA` | Entregue no heartbeat quando o controller avança. |

Resultados sem leitor são descartados. Um snapshot guarda o trabalho que falta, não o histórico. A gravação respeita um intervalo mínimo (`AI_CLUSTER_STATE_WRITE_MS`, 250 ms, nunca abaixo de 9 vezes o tempo da última gravação, o que limita o custo a ~10% do tempo para qualquer tamanho de estado ou disco; por isso não há journal de deltas: o snapshot inteiro continua simples e o custo fica limitado) e um flush por segundo grava o que ficou; SIGTERM ou Ctrl+C gravam antes de sair; um crash (`kill -9`, queda de energia) perde no máximo ~1,3 s de mutações.

## Gravação e leitura

- **Escrita atômica.** Cada arquivo é gravado num irmão temporário e depois renomeado. Uma queda no meio da escrita deixa a gravação anterior intacta.
- **Falha de escrita não é fatal.** O estado em memória continua valendo para a execução.
- **Leitura.** Ao subir, o controller lê o arquivo principal. Se ele estiver ausente, ilegível ou vazio, lê a cópia ao lado. Se nenhum servir, começa vazio, para que um arquivo corrompido não impeça o cluster de aceitar trabalho.
- **Época.** O snapshot guarda uma época monotônica que sobrevive ao reinício. Ela decide qual cópia é mais nova.

## Posse de trabalho

Uma unidade em voo volta ao reinício com o mesmo dono, e o resultado que esse dono entregar depois é aceito. Se o dono não reconectar até a grace contada a partir do início do controller, a unidade volta à fila ou falha, como em uma queda de nó.

A posse é gravada de forma preguiçosa. A atribuição de uma unidade não é gravada sozinha. Se o controller reinicia logo após um despacho, a unidade volta sem dono e pode rodar duas vezes. Ver [[hotplug-e-recuperacao]].

## Recuperação

- **Controller perde o volume.** Se o arquivo local e sua cópia somem, o controller adota a cópia que um worker oferece no re-registro. A adoção só acontece se a cópia for mais nova que a que ele tem e se ele não estiver com trabalho nas mãos.
- **Worker cai.** Suas unidades voltam à fila, dentro do orçamento de tentativas. Ver [[hotplug-e-recuperacao]].

## Configuração

| Variável | Onde | Efeito |
|---|---|---|
| `AI_CLUSTER_STATE` | Controller | Caminho do arquivo de estado. Sem ele, o estado vive só em memória. |
| `AI_CLUSTER_STATE_REPLICA` | Worker | Caminho do arquivo onde o nó guarda a cópia. Sem ele, o nó não guarda cópia. |
| `AI_CLUSTER_EVENTS` | Controller | Caminho do arquivo de eventos. Ver [[observabilidade]]. |
| `AI_CLUSTER_NODES` | Controller | Caminho do arquivo de nós: aprovação, credencial e quarentena sobrevivem a reinício. |

## Garantias

- Um reinício do controller com `AI_CLUSTER_STATE` definido não perde a fila nem os resultados que ainda serão lidos.
- Uma gravação interrompida não corrompe a gravação anterior.
- Um arquivo corrompido não é lido como "nenhum trabalho". Zero bytes conta como arquivo não carregável, e a cópia é tentada.

## Limites conhecidos

- A cópia do controller fica no mesmo volume. Se o volume some, as duas somem juntas. Só a cópia em nó tem outro disco.
- A cópia em nó é o snapshot inteiro, oferecido a cada gravação para cada nó configurado. Não há fator de replicação nem nó designado. Com estado grande, o tráfego cresce com nós e gravações.
- Sem identidade de cluster na cópia: um controller novo apontado para nós com a cópia de outro cluster adota aquela fila.
- A gravação reescreve o estado vivo inteiro a cada alteração, e o custo cresce com o trabalho que falta. Oito resultados de 256 × 256 pendentes já somam cerca de 4 MB por gravação, com a cópia dobrando a escrita. Sem trabalho pendente, o arquivo fica em poucos bytes.
- A posse é gravada de forma preguiçosa (ver acima), e isso pode causar compute duplicado após reinício.
- Não há estado por chave. Não há diretório de referências lógicas (como `state://…`). O KV cache de uma geração de chat vive na memória do gateway e se perde se o gateway cai.
- Não há checkpoint incremental de execução de inferência.
- Não há persistência de reservas nem de prioridade envelhecida (ver [[scheduler]]).
- O arquivo de eventos guarda só os últimos 256 eventos, e não tem `fsync` a cada evento.

> Planejado: diretório de estado com `state://` e localização de réplicas, estado por chave (execução de inferência e KV cache replicado), persistência só da tabela de posse ou de um journal de deltas, fator de replicação com portador designado, métricas de idade do último checkpoint, atraso de réplica e tempo de recuperação.
