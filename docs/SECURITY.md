# Segurança e Confiança

## Estados de confiança

```
UNKNOWN
UNTRUSTED
BOOTSTRAPPED
VALIDATED
TRUSTED
DEGRADED
QUARANTINED
REVOKED
```

Saúde, confiança e estado operacional são atributos distintos.

## Identidade

Cada device possui uma identidade física e cada node possui uma identidade lógica.

Isso permite reprovisionamento sem depender do endereço IP.

## Enrollment

Fluxo:

```
Discovery
→ Bootstrap
→ Enrollment
→ Authentication
→ Validation
→ Trust
```

## Autenticação

O canal Controller ↔ Worker deve usar autenticação mútua e credenciais específicas do cluster.

## Autorização

Um node confiável não recebe acesso irrestrito. O acesso é limitado por:

- operação;
- modelo;
- shard;
- request;
- state object.

## Quarentena

Um node suspeito pode continuar enviando telemetria e diagnóstico, mas fica impedido de executar workload ou receber dados sensíveis.

## Integridade

OS, runtime, modelos e shards devem possuir verificação de versão, integridade e, quando disponível, assinatura.

## Secure Boot

É desejável para hardware compatível, mas não deve excluir todos os equipamentos de reaproveitamento.

## Revogação

Revogar um node deve invalidar credenciais, sessões, leases e autorização de acesso a modelos/estado.

## Princípio de mínimo privilégio

Workers executam somente aquilo que o Scheduler e o Controller autorizaram.

## Auditoria

Eventos de segurança devem ser registrados e relacionados à identidade do node.
