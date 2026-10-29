#!/usr/bin/env bash
# Empacota o worker para um Linux x86_64 com systemd: o binário do worker, o `ggml-rpc-server` do
# llama.cpp (que o bootstrap exige e o operador não precisa mais compilar) e o `bootstrap.sh`, com
# um `install.sh` que junta os três. Os binários saem da imagem única do Compose.
#
#   docker compose -f deploy/docker-compose.yml build      # gera a imagem ai-cluster:local
#   bash code/scripts/package-worker.sh [imagem] [saida]   # padrão: ai-cluster:local, dist/
#
# No destino (Debian/Ubuntu 12+ ou equivalente com glibc 2.36+; precisa de libgomp1):
#   tar xzf ai-cluster-worker-x86_64.tar.gz && cd ai-cluster-worker-x86_64
#   sudo ./install.sh --controller http://controller:50051 --join-token-file /raiz/token \
#        --rpc-tls-dir /etc/ai-cluster/rpc-tls [--node-id nome] [--insecure]
set -euo pipefail
IMAGE=${1:-ai-cluster:local}
OUT=${2:-dist}
HERE=$(cd "$(dirname "$0")/.." && pwd)
NAME=ai-cluster-worker-x86_64
# Dentro de $OUT (caminho relativo): o docker.exe do Windows não enxerga o /tmp do Git Bash.
mkdir -p "$OUT"
STAGE="$OUT/.stage-$$"
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/$NAME"

CID=$(docker create "$IMAGE")
trap 'docker rm -f "$CID" >/dev/null 2>&1; rm -rf "$STAGE"' EXIT
for bin in ai-cluster-worker ggml-rpc-server; do
  docker cp "$CID:/usr/local/bin/$bin" "$STAGE/$NAME/$bin"
done
cp "$HERE/os/bootstrap.sh" "$STAGE/$NAME/bootstrap.sh"
chmod 755 "$STAGE/$NAME/"{ai-cluster-worker,ggml-rpc-server,bootstrap.sh}

cat > "$STAGE/$NAME/install.sh" <<'EOF'
#!/bin/sh
# Instala o ggml-rpc-server em /usr/local/bin e roda o bootstrap com o worker deste pacote.
# Os argumentos vão ao bootstrap.sh (veja o cabeçalho dele): --controller, --join-token-file,
# --rpc-tls-dir, --node-id, --insecure...
set -eu
cd "$(dirname "$0")"
[ "$(id -u)" = 0 ] || { echo "rode como root (sudo)" >&2; exit 1; }
ldconfig -p 2>/dev/null | grep -q libgomp.so.1 || echo "aviso: falta libgomp1 (apt install libgomp1)" >&2
install -m 755 ggml-rpc-server /usr/local/bin/ggml-rpc-server
exec sh ./bootstrap.sh --binary ./ai-cluster-worker --rpc-server-bin /usr/local/bin/ggml-rpc-server "$@"
EOF
chmod 755 "$STAGE/$NAME/install.sh"

# Modo forçado: o NTFS do Windows perde o bit de execução; todos os arquivos do pacote são executáveis.
tar --mode=755 --owner=0 --group=0 -C "$STAGE" -czf "$OUT/$NAME.tar.gz" "$NAME"
echo "pacote: $OUT/$NAME.tar.gz"
tar -tzf "$OUT/$NAME.tar.gz"
