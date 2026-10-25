# AI Cluster OS

Fase F15 ([`fase-15-ai-cluster-os`](../../docs/implementation/15-ai-cluster-os/fase-15-ai-cluster-os.md)). O que existe hoje:

- `bootstrap.sh`: põe o worker num Linux existente (usuário, binário, ambiente 0600, systemd ou segundo plano).
- `Dockerfile.worker`: imagem mínima do worker (`docker build -f os/Dockerfile.worker -t ai-cluster-worker:dev .`, a partir de `code/`).

Não existe imagem de disco, reimage, boot A/B nem ARM64: ver [`f15-bootstrap-e-imagem`](../../docs/implementation/15-ai-cluster-os/f15-bootstrap-e-imagem.md).
