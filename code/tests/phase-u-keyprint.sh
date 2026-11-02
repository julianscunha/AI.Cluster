#!/bin/sh
# Impressão da chave de Linux no install.sh (ADR-031): confere, recusa a diferente, exige parâmetro sem terminal.
# Sem contêiner: extrai confere_impressao do instalador e roda com stubs.
HERE=$(cd "$(dirname "$0")/.." && pwd)
T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
fail() { echo "FALHA: $*"; exit 1; }
echo "chave" > "$T/k.pub"; FP=$(sha256sum "$T/k.pub" | cut -c1-32)
sed -n '/^confere_impressao() {/,/^}/p' "$HERE/os/install.sh" > "$T/fn.sh"
[ -s "$T/fn.sh" ] || fail "confere_impressao não encontrada"
run() { # run <KEYFP> <TRUST_ORIGIN> <YES>
  ( B=""; Z=""; LANG_AIC=pt; KEYFP=$1; TRUST_ORIGIN=$2; YES=$3
    t() { printf '%s' "$1"; }; die() { echo "DIE: $1" >&2; exit 1; }; ok() { :; }; warn() { :; }; have_tty() { return 1; }
    . "$T/fn.sh"; confere_impressao "$T/k.pub" ) >/dev/null 2>"$T/err"
}
run "$FP" 0 1 || fail "impressão certa deveria passar"
run "00000000000000000000000000000000" 0 1 && fail "impressão errada deveria recusar"
run "" 0 1 && fail "sem impressão e sem terminal deveria recusar"
grep -q "key-fingerprint" "$T/err" || fail "a recusa deveria dizer o parâmetro"
run "" 1 1 || fail "--trust-origin-key deveria passar"
echo "OK phase-u-keyprint"
