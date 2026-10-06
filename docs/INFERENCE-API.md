# API de Inferência

## Objetivo

Disponibilizar a capacidade do AI Cluster para aplicações, agentes e orquestradores sem expor a infraestrutura interna.

## Fronteira

```
Aplicação / Agente / Orquestrador
             ↓
        Inference API
             ↓
       AI Cluster
```

Consumidores não acessam diretamente Controller ou Workers.

## Compatibilidade

A interface deve seguir, quando possível, contratos compatíveis com APIs amplamente usadas de inferência, reduzindo esforço de integração.

## Recursos previstos

- chat/completions;
- respostas com streaming;
- embeddings;
- geração;
- outros workloads de IA conforme evolução do runtime.

## Requisição

Deve carregar modelo, mensagens/entrada, prioridade e limites compatíveis com a política do cluster.

## Streaming

Tokens de saída devem poder ser transmitidos incrementalmente ao consumidor.

## Cancelamento

O cliente deve conseguir cancelar uma requisição. O Controller deve propagar cancelamento para Work Units elegíveis.

## Métricas

A API pode retornar identificador da requisição e metadados de consumo conforme autorização:

```
tokens de entrada
tokens de saída
tokens evitados
TTFT
latência
modelo
```

## Erros

Erros externos devem ser estáveis e não expor detalhes internos desnecessários.

## Segurança

A API deve autenticar consumidores e aplicar autorização, limites e políticas de isolamento.

## Independência

A implementação interna pode mudar de forma radical sem exigir mudanças no agente consumidor, desde que o contrato da API permaneça compatível.

## Princípio

O AI Cluster fornece capacidade de IA; agentes continuam responsáveis por interação, ferramentas, lógica de aplicação e experiência do usuário.
