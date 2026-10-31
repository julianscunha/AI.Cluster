---
title: "ADR-026 — Renovação e revogação do certificado do túnel por nó"
aliases: ["ADR-026", "renovação de certificado", "revogação de certificado do nó"]
tags: [sistema/adr, subsistema/seguranca]
type: sistema/adr
---

# ADR-026 — Renovação e revogação do certificado do túnel por nó

Status: decidido · Data: 2026-10-30 · Voltar para [[decisoes]].

## Contexto

O túnel do `rpc-server` (ADR-022) usa um certificado por worker, com o SAN `aic-worker-<nó>` e validade de 825 dias. O convite (`aic cluster invite`) o assina com a CA, que mora no servidor do convite. Faltavam: renovar sem convite aberto, revogar o certificado junto com a credencial do nó e emitir sem convite. Um certificado vazado valia até vencer, mesmo com o nó revogado.

## Decisão

1. **A CA é lida pelo controller, com systemd.** `AI_CLUSTER_RPC_CA_CERT` e `AI_CLUSTER_RPC_CA_KEY` apontam para a CA do cluster. Com systemd, a chave fica `root:ai-cluster 640`: o controller a lê e a unit do gateway esconde `pki/ca.key` (`InaccessiblePaths`). **Sem systemd** (contêiner, OpenRC) o gateway roda como o mesmo usuário do controller, então a chave fica `600 root`, `AI_CLUSTER_RPC_CA_KEY` não é gravado e a renovação fica **desligada**. Com a CA ilegível o controller avisa e sobe mesmo assim.
2. **Renovação pela credencial do nó.** O RPC `RenewNodeCertificate(node_id, csr_pem)` só emite se a credencial presentada é a do nó e o nó está aprovado (`Registry::renews`). O token de join continua no canal, como em toda chamada de nó, mas sozinho não renova nada.
3. **Chave fixada por nó.** Na primeira renovação o controller guarda a impressão digital (SHA-256) da chave pública do CSR no registro do nó (quinta coluna do arquivo de estado). Renovações seguintes com outra chave são recusadas. Nó sem fixação aceita a primeira chave e a grava.
4. **O controller decide o nome.** Ele põe o SAN `aic-worker-<node_id>` (o pedido não manda), CN = `node_id`, `serverAuth`, `CA:FALSE` explícito, 825 dias e serial sorteado. Confere a assinatura do CSR, que prova a posse da chave.
5. **O worker renova sozinho, sem risco de deixar o túnel quebrado.**
   - A renovação grava em `AI_CLUSTER_RPC_TLS_RENEWED_DIR` (o bootstrap usa `<pasta do convite>/renovado`, de dono `ai-cluster`). Nunca mexe na pasta do convite, onde ficam a CA e a chave.
   - O certificado novo vai para `worker.crt.new`, é sincronizado (`fsync`) e só entra se carregar com a chave deste nó. O atual vira `worker.crt.prev` (só se ele também carregava), o novo vira `worker.crt` e o diretório é sincronizado.
   - No boot o túnel usa o primeiro candidato que carrega: `renovado/worker.crt`, `renovado/worker.crt.prev`, depois o certificado do convite. Se o renovado existe e não carrega, o worker cai para o `.prev` e pede renovação na hora.
   - A tarefa de renovação e a de subir o túnel são independentes: se o túnel não sobe (sem certificado que carregue, porta ocupada), o worker segue tentando, e a renovação roda assim mesmo.
   - Um certificado quando vence em menos de 30 dias é renovado; a checagem roda a cada 6 horas (60 segundos enquanto nenhum certificado carrega).
   - Falha de renovação só gera aviso; o certificado em uso segue válido até vencer.
6. **Revogar corta a renovação e o tráfego.** `aic cluster node revoke` já tira a aprovação e troca a credencial. Com isso o nó não renova (`renews` recusa) e sai de `rpc_endpoints` (o gateway só disca para nós aprovados e vivos). A fixação de chave sobrevive à revogação de nó vivo.

## Consequências

- Nó revogado ou pendente não ganha certificado novo e não recebe tráfego do gateway. O certificado já emitido segue válido na criptografia até vencer; quem tiver a chave dele e conseguir se pôr no lugar do endereço do nó ainda passa no TLS.
- **Nó esquecido ou revogado offline precisa de novo convite.** Quando o registro some (o controller esquece o nó), a chave fixada some com ele. Se o nó voltar pelo join e for aprovado, a renovação aceita a primeira chave que apresentar. A regra operacional é tratá-lo como nó novo: novo convite, chave e certificado novos. O controller ainda não impede o retorno com a chave antiga nesse caso (limite abaixo).
- A CA sai do servidor do convite para o controller. Um controller comprometido pode assinar certificados de worker. A troca é aceita porque o controller já é o ponto de confiança do cluster.
- A renovação reaproveita a chave do nó: não há rotação de chave.
- Instalações anteriores não renovam até reinstalar o controller. O `update` não reescreve units nem a permissão da CA de propósito.
- Sem systemd a renovação fica desligada em todo o cluster desse tipo (contêiner, OpenRC). A imagem A/B também não passa `AI_CLUSTER_RPC_TLS_RENEWED_DIR`, então não renova; o certificado do convite segue até vencer.

## Implementação

- `proto/cluster.proto`: `RenewNodeCertificate` no `ClusterControl`, com `RenewNodeCertificateRequest` e `RenewNodeCertificateResponse`.
- Controller: `certs.rs` (`CaSigner`, `sign_worker`, `key_fingerprint`, leitura do ambiente), `Registry::renews` e `Registry::bind_key`, o handler em `service.rs` e a ligação em `main.rs`.
- Túnel (`ai-cluster-tunnel`): `serve` recebe um `watch` com a configuração TLS (`Tls`), lido em cada conexão; a troca vale na conexão seguinte.
- Worker: `cert.rs` (CSR, prazo, pedido, conferência, escrita atômica com `.prev`, candidatos) e `rpc.rs` (`tunnel` e `choose`, com a renovação independente do túnel).
- Instalador: chave da CA com o grupo só com systemd; `AI_CLUSTER_RPC_CA_*` no `controller.env` só com systemd; `InaccessiblePaths` do gateway com a chave; pasta `renovado` com dono e `ReadWritePaths` (`bootstrap.sh`).
- Testes: unidade de emissão (SAN forçado, EKU, `CA:FALSE` explícito, prazo, recusas, chave da CA trocada), de `renews` e `bind_key` (pendente, revogado, desconhecido, credencial errada, fixação), do serviço (sem credencial, sem CA, revogado, outra chave), do CSR, do prazo e da troca atômica com `.prev` no worker, e da troca do certificado no túnel. O `phase-u-setup.sh` confere a permissão da CA com e sem systemd, a unit do gateway e a variável no `controller.env`.

## Alternativas

- **Endpoint permanente no servidor do convite:** descartado. Seria outro processo de rede com a CA, e ainda assim precisaria validar a credencial no controller.
- **Renovar só com convite aberto:** é o que havia, e não atende.
- **CA fora do controller (helper root, HSM):** mais código e mais um processo; fica para quando houver quem exija.
- **Lista de revogação (CRL) consultada no TLS:** adiada. O controller é a única fonte de endereços do gateway, então a revogação já corta o tráfego.
- **Trocar a chave do nó na renovação:** adiado. A chave atual não sai do worker, e trocá-la exigiria mexer no arquivo do grupo.
- **Gravar o renovado na pasta do convite:** descartado. A pasta tem a CA e a chave; dar escrita a ela deixaria o worker trocar a própria CA de confiança.

## Limites

- Sem CRL: o certificado do nó revogado segue válido na criptografia até vencer (até 825 dias). A proteção é o controller não listar o nó nem renovar.
- Nó esquecido ou revogado offline volta a ser um nó sem fixação (ver Consequências); o correto é novo convite, e isso é regra de operação, não imposta pelo código.
- A renovação reaproveita a chave; rotação de chave do nó não existe.
- Não há rotação da CA: trocar a CA exige reemitir todos os certificados (reinstalar).
- Instalações anteriores e instalações sem systemd não renovam.
- A renovação ponta a ponta (worker chamando o controller real, gravando e recarregando o túnel) não foi testada em processos reais; testada por unidade e pelo `phase-u-setup.sh` na parte de instalação. Não foi exercitada em hardware (Orange Pi, imagem A/B).

Ver também [[adr-016-mtls-opcional-no-plano-de-controle]] e [[adr-022-fluxo-de-modelos-e-motor-hibrido]].
