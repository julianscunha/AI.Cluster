# Política de segurança

## Reportar uma vulnerabilidade

**Não abra issue pública.** Use o relato privado do GitHub: aba *Security* → *Report a vulnerability* (<https://github.com/julianscunha/Real.AI.Cluster/security/advisories/new>). Inclua versão/commit, componente, passos para reproduzir e impacto.

Respondo em até 7 dias; a correção e a divulgação são combinadas com quem relatou. O projeto é mantido por uma pessoa, sem SLA formal.

## Escopo

Controller, worker, gateway, CLI, protocolo gRPC (`code/proto/cluster.proto`) e a imagem AI Cluster OS (updater, release assinada, rollout).

## Limites conhecidos (não são vulnerabilidades novas)

Já documentados, com o raciocínio nos ADRs:

- O cluster de desenvolvimento (`dev-cluster`) roda sem TLS e com tokens fixos: é só para a máquina local.
- A *attestation* é por medida de software: um nó comprometido mente. Sem TPM, secure boot nem dm-verity ([ADR-018](docs/system/05-decisoes/adr-018-release-assinada-e-medida-do-slot.md)).
- Sem lista de revogação de chave de release; a rotação é por imagem nova.
- O download da imagem usa http (o SHA-256 e a assinatura limitam o dano).

Quem tem a chave privada de release ou o token de operador controla o cluster; guarde-os fora do repositório.

## Versões suportadas

Só o `main`. O projeto está em pré-produção.
