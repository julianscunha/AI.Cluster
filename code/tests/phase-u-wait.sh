#!/usr/bin/env bash
# Instalação com aprovação do controller (ADR-023), primeira etapa: o aparelho em espera
# (`aic-wait`) aparece em `ai-cluster installs`, o operador aprova com o disco e o aparelho
# recebe o token; o MAC aprovado não é tomado por quem repete o MAC com outro segredo.
#
# Roda tudo num contêiner só (controller, aic-wait e CLI), compilando em debug no volume
# aic-target-test. Sem TLS (rede de teste).
set -uo pipefail

MSYS_NO_PATHCONV=1 docker run --rm -v "$(cd "$(dirname "$0")/.." && pwd -W 2>/dev/null || pwd):/w" \
  -v aic-target-test:/t -v aic-cargo-registry:/usr/local/cargo/registry \
  -e CARGO_TARGET_DIR=/t -w /w aic-rust:1 sh -c '
set -u
cargo build -q -p ai-cluster-controller -p ai-cluster-cli -p ai-cluster-wait || exit 1
B=/t/debug
export AI_CLUSTER_INSECURE=1 AI_CLUSTER_JOIN_TOKEN=u-join AI_CLUSTER_OPERATOR_TOKEN=u-op AI_CLUSTER_INFERENCE_TOKEN=u-inf
export AI_CLUSTER_CONTROLLER=http://127.0.0.1:50051 AI_CLUSTER_METRICS_ADDR=127.0.0.1:19101
AI_CLUSTER_ADDR=127.0.0.1:50051 AI_CLUSTER_ENROLL_ADDR=127.0.0.1:50052 $B/ai-cluster-controller >/tmp/controller.log 2>&1 &
CTRL=$!
fail() { echo "FALHA: $1"; cat /tmp/controller.log | tail -5; kill $CTRL 2>/dev/null; exit 1; }
for i in 1 2 3 4 5 6 7 8 9 10; do grep -q "etapa de espera" /tmp/controller.log && break; sleep 0.5; done

# 0) a porta nasce fechada: o aparelho recebe "indisponível" e nada entra na lista
$B/aic-wait http://127.0.0.1:50052 --once 2>&1 | grep -q "instalação fechada" || fail "a porta devia nascer fechada"
$B/ai-cluster installs | grep -q "nenhum aparelho" || fail "algo entrou com a porta fechada"
$B/ai-cluster enrollment open --minutes 5 | grep -q aberta || fail "enrollment open"
echo "OK 0) porta fechada por padrão e aberta pelo operador"

# A) o aparelho se apresenta e fica na lista, aguardando
OUT=$($B/aic-wait http://127.0.0.1:50052 --once) || fail "aic-wait --once"
FP=$(echo "$OUT" | sed -n "s/.*fingerprint=\([0-9a-f-]*\) approved=false.*/\1/p")
[ -n "$FP" ] || fail "sem impressão digital: $OUT"
$B/ai-cluster installs | grep -q "fingerprint=$FP aguardando" || fail "installs não lista $FP"
echo "OK A) aparelho aguardando ($FP)"

# B) disco que o aparelho não informou é recusado; o token só vem depois da aprovação
$B/ai-cluster approve-install "$FP" --disk nao-existe >/dev/null 2>&1 && fail "aprovou disco inexistente"
DISK=$($B/ai-cluster installs | sed -n "s/.*discos=\([a-z0-9]*\)=.*/\1/p" | head -1)
[ -n "$DISK" ] || fail "o aparelho não informou disco"
echo "OK B) disco inexistente recusado (disco real: $DISK)"

# C) com a aprovação, o MESMO aparelho (mesmo segredo) recebe o token; outro processo não
#    herda a aprovação. Como o segredo nasce a cada execução, o teste fica com um aic-wait vivo.
rm -f /tmp/install.json
AIC_INSTALL_OUT=/tmp/install.json $B/aic-wait http://127.0.0.1:50052 >/tmp/wait.log 2>&1 </dev/null &
WAIT=$!
sleep 1
FP2=$($B/ai-cluster installs | sed -n "s/.*fingerprint=\([0-9a-f-]*\) aguardando.*/\1/p" | grep -v "^$FP$" | head -1)
[ -n "$FP2" ] || fail "o segundo aparelho não apareceu (mesmo MAC no mesmo IP troca a entrada: era esperado FP novo)"
$B/ai-cluster approve-install "$FP2" --disk "$DISK" | grep -q aprovada || fail "approve-install"
for i in 1 2 3 4 5 6 7 8 9 10 11 12; do [ -s /tmp/install.json ] && break; sleep 1; done
[ -s /tmp/install.json ] || fail "o aparelho não recebeu o token: $(tail -3 /tmp/wait.log)"
grep -q "\"disk\":\"$DISK\"" /tmp/install.json || fail "disco errado no arquivo"
TOKEN=$(sed -n "s/.*\"token\":\"\([0-9a-f]*\)\".*/\1/p" /tmp/install.json)
[ ${#TOKEN} -eq 32 ] || fail "token inesperado: $TOKEN"
echo "OK C) o dono do segredo recebeu o token e o disco ($DISK)"

# D) quem aparece depois com o mesmo MAC (outro segredo) não derruba a aprovação
OUT=$($B/aic-wait http://127.0.0.1:50052 --once 2>&1)
echo "$OUT" | grep -qiE "já foi aprovado|already" || fail "MAC aprovado foi tomado: $OUT"
echo "OK D) MAC aprovado não é tomado"

# E) recusar tira da lista
$B/ai-cluster reject-install "$FP2" | grep -q recusada || fail "reject-install"
$B/ai-cluster installs | grep -q "$FP2" && fail "ainda listado"
echo "OK E) recusar remove da lista"

$B/ai-cluster enrollment close | grep -q fechada || fail "enrollment close"
$B/aic-wait http://127.0.0.1:50052 --once 2>&1 | grep -q "instalação fechada" || fail "fechar não vale"
echo "OK F) fechar a porta vale na hora"

kill $CTRL $WAIT 2>/dev/null
echo "TUDO OK"
'
