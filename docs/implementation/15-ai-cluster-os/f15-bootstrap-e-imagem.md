---
title: "F15 — Bootstrap e imagem mínima do worker"
aliases: ["J1", "J2", "J3", "bootstrap.sh", "Dockerfile.worker"]
tags: [implementacao/fase/15, implementacao/fatia, implementacao/estado/parcial]
type: implementacao/fatia
status: parcial
superado: false
fase: "[[fase-15-ai-cluster-os]]"
codigos-antigos: ["J1", "J2", "J3"]
sistema: ["[[ai-cluster-os]]", "[[provisionamento]]"]
evidencia: ["code/tests/phase-p-bootstrap.sh", "code/tests/phase-p-image.sh"]
---
# F15 — Bootstrap e imagem mínima do worker

- [ ] Superado

## Objetivo

O gate da F15 é "uma máquina limpa vira worker READY sem operação manual". Sem VM nem hardware nesta etapa, dá para provar a parte que não depende de boot: instalar o worker num Linux que já existe (J3) e ter uma imagem mínima que carrega só o worker (a versão em contêiner de J1/J2).

## O que foi feito

- **`code/os/bootstrap.sh`** (J3). Num Linux existente, como root: `--controller URL --join-token-file ARQ [--binary] [--node-id] [--prefix] [--model-dir] [--insecure]`. Cria o usuário de sistema `ai-cluster`, instala o binário em `<prefix>/bin`, grava `<prefix>/worker.env` como `root:ai-cluster` 0640 (o serviço lê o token e não o reescreve) e sobe o worker: unit systemd (`NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome`, escrita só em `state/`) quando há systemd, senão em segundo plano com log e pid em `state/`. `http://` exige `--insecure`. Todo valor (controller, token, id, prefixo, diretório de modelos) é validado contra uma lista de caracteres permitidos (letras, dígitos e `._:/@=+-`) antes de qualquer escrita, porque entra em arquivo de ambiente, unit e comando; prefixo e diretório de modelos devem ser absolutos. `--join-token-file` evita pôr o segredo na linha de comando. O prefixo e o binário são do root (o worker não troca o próprio executável), só `state/` é do worker, e um prefixo que é link ou não é do root é recusado. Trocar o `--node-id` de uma máquina já instalada exige `--force`. A unit tem `NoNewPrivileges`, `ProtectSystem=strict`, `PrivateTmp`, `PrivateDevices`, `Protect*`, `RestrictAddressFamilies`, `RestrictSUIDSGID`, `CapabilityBoundingSet=` vazio e escrita só em `state/`. Rodar de novo atualiza binário e ambiente e troca o processo (idempotente). Não aprova o nó: com aprovação manual o operador ainda roda `ai-cluster approve`.
- **`code/os/Dockerfile.worker`** (J1/J2 em contêiner). Dois estágios: compila `ai-cluster-worker` com `--locked` e copia só o binário para um `debian:bookworm-slim` com `ca-certificates` e o usuário `ai-cluster` (UID 10001). A imagem tem 33 MB, não traz configuração (controller, token e id entram por variáveis) e guarda a credencial do nó em um volume.

## Evidência

- `bash code/tests/phase-p-bootstrap.sh`: um Debian limpo (sem Rust, nada do cluster) recebe só o binário e o script e vira `node-clean` READY; `worker.env` sai 640 `root:ai-cluster` e o binário 755 do root, o processo roda como `ai-cluster`; argumentos ruins (inclusive `$(…)`, crases, `\`, `#`, `;` e quebra de linha em token, id, prefixo e diretório de modelos) são recusados com código 2 antes de tocar na máquina; trocar o id sem `--force` também; uma segunda execução deixa um nó e um processo.
- `bash code/tests/phase-p-image.sh`: constrói a imagem do zero (33 MB, usuário `ai-cluster`) e o contêiner vira `node-image` READY.

## Limites

> [!warning] Isto não é o gate da F15
> Nada aqui instala um sistema operacional: não há imagem de disco ou ISO, reimage (J4), boot e recuperação A/B (J5) nem ARM64 (J6). O bootstrap parte de um Linux que alguém já instalou. O teste do systemd é só de leitura da unit: nenhum teste roda com systemd de verdade (o contêiner de teste não tem). O binário é o da máquina de build (glibc); sem assinatura nem verificação de origem. O token de join fica em texto no `worker.env` e na variável de ambiente da imagem. As imagens-base não são fixadas por digest. Enrollment automático com aprovação não existe.

> [!todo] O que falta para o gate
> Uma VM ou máquina para o que não cabe em contêiner: imagem de disco, instalador, reimage, A/B, ARM64. Isso exige hardware ou virtualização que este ambiente não tem. M2 segue aberto.

## Próximos passos

- Testar `bootstrap.sh` em uma VM com systemd.
- Imagem de disco (Debian mínimo ou Alpine) gerada por script, e instalador.
