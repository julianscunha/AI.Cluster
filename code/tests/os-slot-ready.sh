#!/usr/bin/env bash
# Critério do slot por papel (usr/lib/aic/slot-ready.sh), sem hardware: bash tests/os-slot-ready.sh
set -uo pipefail
O=$(cd "$(dirname "$0")/../os/image/overlay" && pwd)
T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
fail() { echo "FALHA: $1"; exit 1; }
sh -n "$O/usr/lib/aic/slot-ready.sh" && sh -n "$O/etc/init.d/aic-slot-commit" || fail "sh -n"
printf '#!/bin/sh
[ "$5" = "${NC_OPEN:-}" ]
' > "$T/nc"; chmod +x "$T/nc"
export AIC_ROLE_FILE=$T/role AIC_WORKER_LOG=$T/log AIC_NC=$T/nc
r() { env NC_OPEN="${NC_OPEN:-}" sh -c ". '$O/usr/lib/aic/slot-ready.sh'; slot_ready"; }
# worker (sem arquivo de papel): vale o registro no log, a porta não conta
r && fail "worker pronto sem registro"
echo "registrado node_id=x aceito=true" > "$T/log"; r || fail "worker registrado deveria estar pronto"
# controller: registro no log não basta; a porta tem de responder
echo controller > "$T/role"
NC_OPEN= r && fail "controller pronto com a porta fechada"
NC_OPEN=50051 r || fail "controller com a porta 50051 aberta deveria estar pronto"
echo 7000 > "$T/role.port"; NC_OPEN=50051 r && fail "porta configurada ignorada"
NC_OPEN=7000 r || fail "porta configurada 7000 aberta"
rm "$T/role.port"
# gateway: 8080 por padrão
echo gateway > "$T/role"; NC_OPEN=8080 r || fail "gateway 8080"
NC_OPEN=50051 r && fail "gateway não usa a porta do controller"
echo "OK: critério do slot por papel"
