# AI Cluster — Distributed AI Runtime

Plataforma Linux-first que transforma hardware heterogêneo reaproveitado em um pool distribuído de computação para IA. Uma única inferência pode ser dividida entre vários dispositivos (x86_64, ARM64, CPU, GPU, NPU; PCs, servidores, notebooks, SBCs e celulares compatíveis), com hot-plug, drain, falha e replanejamento sem derrubar o cluster.

O que existe hoje (Rust, `code/`): controller e workers com join seguro, capability/health, DAG com retry e placement por custo, observabilidade, estado persistente, papéis de credencial, e uma Inference API (`/v1/chat/completions`, SSE) sobre GPT-2 e SmolLM2 com as projeções fatiadas nos workers. O restante é arquitetura-alvo; o estado exato está em [`docs/implementation/status.md`](docs/implementation/status.md).

## Documentação

Toda a documentação é um *vault* do Obsidian em `docs/`, em duas pastas:

- [`docs/system/`](docs/system/system.md) — o sistema e como ele funciona, na totalidade, inclusive o que não está implementado: visão, princípios, requisitos, arquitetura, subsistemas, protocolo, modelo de dados e decisões (ADRs).
- [`docs/implementation/`](docs/implementation/implementation.md) — planejamento e implementação em fases `F00`–`F19`, com slices, status, testes e evidências.

Comece por [`docs/inicio.md`](docs/inicio.md). Convenções de formato: [`docs/convencoes.md`](docs/convencoes.md). Nos documentos, os links são wikilinks do Obsidian; abra a pasta do repositório como vault.

## Código

- `code/` — workspace Rust (comandos de build e teste em [`AGENTS.md`](AGENTS.md) e [`code/README.md`](code/README.md)).
- `code/proto/cluster.proto` — contrato de wire, pacote `ai.cluster.v1`.
- `code/tests/` — suíte E2E em shell (mapa em [`docs/implementation/transversal/evidencias-e2e.md`](docs/implementation/transversal/evidencias-e2e.md)).

## Regra operacional

Nenhum detalhe de implementação é definitivo só por ter sido proposto. Quando uma decisão for validada por benchmark, teste ou experimento, atualize o documento correspondente e registre o ADR.
