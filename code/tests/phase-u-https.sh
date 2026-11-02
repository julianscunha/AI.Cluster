#!/bin/sh
# Download por HTTPS (pendências, Lote 3): origem https só fala https, inclusive nos redirecionamentos;
# origem http (lab) continua sem restrição. Estático, sem contêiner.
HERE=$(cd "$(dirname "$0")/.." && pwd)
fail() { echo "FALHA: $*"; exit 1; }
for f in install.sh setup; do
  n=$(grep -c 'PROTO=""; case "' "$HERE/os/$f"); [ "$n" -ge 1 ] || fail "$f sem PROTO por origem"
  grep 'PROTO=""; case "' "$HERE/os/$f" | grep -vq -- '--proto-redir =https' && fail "$f: PROTO sem --proto-redir"
  grep 'PROTO=""; case "' "$HERE/os/$f" | grep -vq 'https://\*)' && fail "$f: PROTO não depende do esquema"
done
# fetch_worker_pack baixa SHA256SUMS, pacote e assinatura com $PROTO
sed -n '/^fetch_worker_pack() {/,/^}/p' "$HERE/os/setup" | grep 'curl ' | grep -v '\$PROTO' && fail "fetch_worker_pack tem curl sem \$PROTO"
echo "OK phase-u-https"
