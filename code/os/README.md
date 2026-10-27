# AI Cluster OS

Ferramentas para pôr um worker numa máquina:

- `bootstrap.sh`: instala o worker num Linux existente (usuário, binário, ambiente 0600, systemd ou segundo plano).
- `Dockerfile.worker`: imagem mínima do worker (`docker build -f os/Dockerfile.worker -t ai-cluster-worker:dev .`, a partir de `code/`).
- `image/`: imagem Alpine imutável com slots A/B para SBCs ARMv7, instalador de disco, release assinada.
- `netboot/`: recuperação por boot de rede.

Instalação e operação: [`docs/guia/instalacao.md`](../../docs/guia/instalacao.md) e [`docs/guia/operacao.md`](../../docs/guia/operacao.md).
