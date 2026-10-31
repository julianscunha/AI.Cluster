---
title: "Guia de uso e operação"
aliases: ["Guia", "Índice do guia"]
tags: [guia]
type: guia
---

# Guia de uso e operação

Notas práticas para instalar, configurar, operar e testar o AI Cluster. Cada nota descreve o que o código faz hoje; quando algo ainda não existe, a nota diz isso.

| Nota | Para quê |
|---|---|
| [[instalacao]] | Compilar, subir um cluster de desenvolvimento, montar um cluster com TLS, instalar workers e a imagem do AI Cluster OS |
| [[configuracao]] | Todas as variáveis `AI_CLUSTER_*` por componente, com padrão e efeito |
| [[cli]] | Comando `aic`: gramática, subcomandos, flags e migração dos nomes antigos |
| [[api]] | API HTTP do gateway: rotas, autenticação, cabeçalhos, cotas e erros |
| [[modelos]] | Modelos suportados e homologados, do mais simples ao mais avançado, com notas e links de download |
| [[operacao]] | Tokens e papéis, mTLS, aprovação de workers, saída de workers, eventos, métricas, estado, atualização assinada, rotação de chave e solução de problemas |
| [[testes]] | Testes unitários, de paridade e a suíte E2E |

## Componentes

- `ai-cluster-controller`: plano de controle. Registra workers, guarda a fila de trabalho, decide o placement e expõe o gRPC de controle.
- `ai-cluster-worker`: agente do worker. Registra-se, envia heartbeat, mede a capacidade e executa unidades de trabalho.
- `ai-cluster-gateway`: API HTTP de inferência (`/v1/chat/completions`, `/v1/embeddings`, `/v1/models`).
- `aic`: o comando único do operador e de cada máquina (cluster, controller, gateway, worker e atualização). Fala com o controller.

## Ordem sugerida

1. Compile e suba o cluster de desenvolvimento ([[instalacao]]).
2. Leia as variáveis que a sua topologia precisa ([[configuracao]]).
3. Em produção, configure TLS e tokens separados, e aprove os workers ([[operacao]]).
4. Use a API ([[api]]) ou a CLI ([[cli]]) para acompanhar o cluster.
