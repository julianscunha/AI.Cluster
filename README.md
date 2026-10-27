<p align="center">
  <img src="docs/assets/banner.svg" alt="AI Cluster" width="100%">
</p>

<p align="center">
  <a href="https://github.com/julianscunha/Real.AI.Cluster/actions/workflows/rust.yml"><img alt="CI" src="https://github.com/julianscunha/Real.AI.Cluster/actions/workflows/rust.yml/badge.svg"></a>
  <a href="LICENSE"><img alt="Licença" src="https://img.shields.io/badge/licen%C3%A7a-Apache--2.0-blue"></a>
  <img alt="Rust 2024" src="https://img.shields.io/badge/rust-edition%202024-orange?logo=rust">
  <img alt="Plataformas" src="https://img.shields.io/badge/x86__64%20%7C%20ARMv7-testado-informational">
  <img alt="Estado" src="https://img.shields.io/badge/estado-pr%C3%A9--produ%C3%A7%C3%A3o-yellow">
  <a href="SECURITY.md"><img alt="Segurança" src="https://img.shields.io/badge/seguran%C3%A7a-pol%C3%ADtica-green"></a>
</p>

# AI Cluster — Distributed AI Runtime

Runtime Linux-first, escrito em Rust, que junta hardware heterogêneo reaproveitado (PCs, notebooks, servidores, SBCs como Orange Pi) num **pool único de computação para IA**. Uma inferência pode ser dividida entre vários dispositivos; nós entram, saem, falham e são drenados sem derrubar o cluster.

## Propósito

Muita máquina parada serve para IA se for orquestrada: cada uma é fraca, o conjunto não. O projeto resolve o que falta entre "tenho dez máquinas velhas" e "tenho um serviço de inferência": descoberta e admissão seguras de nós, medição do que cada um aguenta, colocação de trabalho por custo, tolerância a falha, observabilidade e atualização do próprio sistema operacional dos nós.

## O que ele faz

| Função | Como |
|---|---|
| **Cluster** | `controller` mantém registry de nós, work store, reservas e eventos; `worker` é o agente do nó (join com token, heartbeat, drain, saída limpa). |
| **Capability e placement** | Cada nó mede CPU, memória, rede e disco; o controller decide onde cada Work Unit roda, por custo, com DAG, retry e replanejamento. |
| **Inferência distribuída** | `gateway` expõe `/v1/chat/completions` (SSE) sobre GPT-2 e SmolLM2; o `engine` fatia as projeções de peso entre workers, que as mantêm residentes. Se um worker cai, as faixas são reatribuídas. Paridade numérica com `transformers` é critério de aceite. |
| **Segurança por papéis** | Três credenciais (join, operador, inferência); mTLS opcional; o gateway roda sem o token de operador. |
| **Observabilidade** | Métricas Prometheus, eventos, traces e dashboard na CLI. |
| **AI Cluster OS** | Imagem Alpine A/B para SBCs com boot por u-boot, rollback automático, **release assinada (Ed25519)** conferida no nó, rollout com canário pelo controller e medida SHA-256 do slot como attestation de software. |

O estado exato (o que existe, o que é arquitetura-alvo) está em [`docs/implementation/status.md`](docs/implementation/status.md). Limites de segurança conhecidos estão nos ADRs, por exemplo [ADR-018](docs/system/05-decisoes/adr-018-release-assinada-e-medida-do-slot.md).

## Arquitetura em uma figura

```
                 operador (ai-cluster CLI)
                          │  gRPC, token de operador
                          ▼
   cliente ──HTTP──► gateway ──gRPC──► controller ◄──gRPC── workers (x86, ARM…)
 (/v1/chat/…)        │  engine           registry · placement     benchmark · ShardMatmul
                     └─ fatias de peso ─────────────────────────► residentes nos workers
```

## Como iniciar

Requisitos: Rust estável (edition 2024) **ou** Docker. `protoc` vem vendorizado.

```bash
git clone https://github.com/julianscunha/Real.AI.Cluster.git
cd Real.AI.Cluster/code

cargo test --workspace --all-features          # unitários
bash scripts/dev-cluster.sh build              # compila no volume Docker aic-target
bash scripts/dev-cluster.sh up 2               # controller + 2 workers (sem TLS, só para desenvolvimento)
bash scripts/dev-cluster.sh cli nodes          # nós e saúde
bash scripts/dev-cluster.sh cli dashboard --watch 2
bash scripts/dev-cluster.sh down
```

No Windows use `scripts/dev-cluster.ps1`. Os tokens do cluster de desenvolvimento são fixos (`dev-token`, `dev-operator`): **nunca** exponha esse cluster numa rede aberta.

Testes de ponta a ponta: `bash tests/<script>.sh` (mapa em [`docs/implementation/transversal/evidencias-e2e.md`](docs/implementation/transversal/evidencias-e2e.md)). Os de inferência exigem os pesos em `code/models/{gpt2,smollm2,smollm2-instruct}` (veja [`f08-engine-local.md`](docs/implementation/08-inferencia/f08-engine-local.md)).

Para instalar o sistema num nó real (imagem A/B, rollout assinado): [`provisionamento`](docs/system/02-subsistemas/plataforma/provisionamento.md) e [`f19-rollout-de-release`](docs/implementation/19-hardening-de-producao/f19-rollout-de-release.md).

## Estrutura

```
code/
  crates/{common,protocol,controller,worker,transport,benchmark,cli,engine,gateway}
  proto/cluster.proto      contrato de wire (ai.cluster.v1)
  os/                      imagem A/B, netboot, instalador
  tests/ scripts/          E2E em shell, cluster de desenvolvimento
docs/                      vault Obsidian: system/ (o sistema) e implementation/ (fases F00–F19)
```

## Documentação

Todo o conhecimento do projeto é um *vault* do Obsidian em `docs/`. Comece por [`docs/inicio.md`](docs/inicio.md); convenções em [`docs/convencoes.md`](docs/convencoes.md). Layout dos crates e armadilhas: [`AGENTS.md`](AGENTS.md).

## Contribuir, segurança e licença

- Contribuição: [`CONTRIBUTING.md`](CONTRIBUTING.md) · conduta: [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md)
- Vulnerabilidades: [`SECURITY.md`](SECURITY.md) (não abra issue pública)
- Licença: [Apache-2.0](LICENSE)

> Nenhum detalhe de implementação é definitivo só por ter sido proposto. Quando uma decisão for validada por benchmark, teste ou experimento, atualize o documento correspondente e registre o ADR.
