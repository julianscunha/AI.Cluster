---
title: "F19 — Upgrade e rollback do worker"
aliases: ["upgrade", "rollback", "gate de saúde"]
tags: [implementacao/fase/19, implementacao/fatia, implementacao/estado/parcial]
type: implementacao/fatia
status: parcial
superado: false
fase: "[[fase-19-hardening-de-producao]]"
sistema: ["[[ai-cluster-os]]"]
evidencia: ["code/os/test-upgrade.sh"]
---
# F19 — Upgrade e rollback do worker

- [ ] Superado

## Objetivo

Trocar a versão do worker numa máquina sem derrubá-la: a versão nova só fica se o nó voltar saudável, senão a anterior volta sozinha.

## O que existe

- `os/bootstrap.sh`, rodado de novo com outro `--binary`, guarda a versão em uso em `bin/ai-cluster-worker.prev` e instala a nova.
- Com systemd, a nova só vale se o serviço ficar ativo **e** o worker se registrar no controller (`registrado ... aceito=true`) em `--health-timeout` segundos (padrão 90; 0 desliga a espera). Senão o `.prev` volta, o serviço reinicia e o script sai com código 3.
- `os/test-upgrade.sh` (roda no destino): atualizar com binário bom deixa o serviço ativo e registrado; atualizar com binário quebrado sai com 3, mantém o binário bom (mesmo SHA-256) e o nó volta a se registrar. Validado num Orange Pi PC (Ubuntu 18.04, systemd 237, armv7).

## Limites

- **Não é automático.** Quem dispara é uma pessoa rodando o script na máquina. O controller não conhece a versão de cada nó, não publica release nem empurra atualização. É o bloco que um rollout automático vai usar.
- Só uma versão de volta (`.prev`); sem histórico.
- Sem systemd (contêiner), o script não espera nem faz rollback.
- Atualiza só o binário do worker; o sistema operacional (reimage, A/B) é a F15 J4/J5.
- A saúde é "registrou"; um binário que registra e quebra depois não é pego pelo gate.
