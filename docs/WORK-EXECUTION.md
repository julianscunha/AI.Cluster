# Execução Inicial de Work Unit

## Objetivo

Validar o primeiro caminho completo de trabalho:

```
Cliente
→ Controller
→ seleção de Worker
→ Worker Runtime
→ execução
→ resultado
```

## Contrato

A Work Unit possui, na primeira implementação:

- identificador;
- operação;
- payload;
- prioridade.

## Operações iniciais

### echo

Retorna o payload sem transformação. Serve para validar transporte e integridade básica.

### cpu_spin

Executa uma quantidade definida de iterações de CPU.

O número de iterações é enviado como inteiro sem sinal de 64 bits em little-endian.

## Seleção inicial

Nesta etapa o Controller escolhe um Worker elegível sem ainda aplicar o Scheduler completo.

O objetivo é validar a chamada remota e criar a base para o Scheduler.

## Limitações

Esta implementação ainda não divide uma operação entre múltiplos Workers.

O próximo incremento deverá adicionar particionamento de uma operação matricial, permitindo comparar execução local com execução distribuída.

## Métricas

A execução retorna:

- node responsável;
- operação;
- duração em microssegundos;
- tamanho do resultado.

Essas métricas serão posteriormente incorporadas ao sistema de telemetria.
