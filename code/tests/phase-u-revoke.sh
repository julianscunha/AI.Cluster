#!/bin/sh
# Lista de revogação da chave de Linux (ADR-031, item 7): extrai as funções do setup e as roda contra uma "release"
# em file://. Sem contêiner. Precisa de OpenSSL 3.
HERE=$(cd "$(dirname "$0")/.." && pwd)
T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
fail() { echo "FALHA: $*"; exit 1; }
openssl version | grep -Eq '^OpenSSL (3|[4-9])' || { echo "pulado: precisa de OpenSSL 3"; exit 0; }
for f in chave_fp revogada confere_assinatura busca_rotacao busca_revogacao; do
  sed -n "/^$f() {/,/^}/p" "$HERE/os/setup" >> "$T/fn.sh"
done
grep -q busca_revogacao "$T/fn.sh" || fail "funções não encontradas no setup"
for k in o n; do openssl genpkey -algorithm ed25519 -out "$T/$k.key" 2>/dev/null; openssl pkey -in "$T/$k.key" -pubout -out "$T/$k.pub"; done
sig() { openssl pkeyutl -sign -inkey "$T/$1.key" -rawin -in "$2" -out "$T/s.bin" && base64 -w0 < "$T/s.bin" > "$2.sig"; }
FPO=$(sha256sum "$T/o.pub" | cut -c1-32); FPN=$(sha256sum "$T/n.pub" | cut -c1-32)
# release <pasta> <chave-que-assina-a-lista> <rotaciona 0|1>: monta uma release assinada pela chave antiga (o)
release() {
  R=$1; rm -rf "$R"; mkdir -p "$R"; echo "9.9.9" > "$R/VERSION"; echo "$FPO" > "$R/revoked.txt"; sig "$2" "$R/revoked.txt"
  [ "$3" = 1 ] && { cp "$T/n.pub" "$R/release.pub.next"; sig o "$R/release.pub.next"; }
  ( cd "$R" && for f in VERSION revoked.txt revoked.txt.sig release.pub.next release.pub.next.sig; do [ -f "$f" ] && printf '%s  %s\n' "$(sha256sum "$f" | cut -d' ' -f1)" "$f"; done > SHA256SUMS ); sig o "$R/SHA256SUMS"
}
run() { # run <release> -> mesmo shell; imprime o status das duas buscas
  ( CONF="$T/conf"; PROTO=""; LANG_AIC=pt; t() { printf '%s' "$1"; }
    . "$T/fn.sh"; W="$T/w"; rm -rf "$W"; mkdir -p "$W"
    confere_assinatura "$1/SHA256SUMS" "$1/SHA256SUMS.sig" || { echo "sums: $SIG_MOTIVO"; exit 1; }
    busca_rotacao "$W" "file://$(cygpath -m "$1" 2>/dev/null || echo "$1")" "$1/SHA256SUMS" || { echo "rot: $SIG_MOTIVO"; exit 1; }
    busca_revogacao "$W" "file://$(cygpath -m "$1" 2>/dev/null || echo "$1")" "$1/SHA256SUMS" || { echo "crl: $SIG_MOTIVO"; exit 1; }
    cat "$W/revoked.new" 2>/dev/null; echo "ROT=$ROT" )
}
mkdir -p "$T/conf"; cp "$T/o.pub" "$T/conf/release.pub"
# 1) rotação para n, lista assinada por n (a chave rotacionada) revoga o: aceita
release "$T/r1" n 1; out=$(run "$T/r1") || fail "rotação com lista da chave nova deveria passar: $out"
printf '%s\n' "$out" | grep -qx "$FPO" || fail "a lista deveria trazer a chave antiga"; printf '%s\n' "$out" | grep -q "ROT=1" || fail "ROT=1"
# 2) rotação, mas a lista foi assinada pela chave antiga: recusa
release "$T/r2" o 1; out=$(run "$T/r2") && fail "lista assinada pela chave que ela revoga deveria ser recusada"; echo "$out" | grep -q "^crl:" || fail "deveria recusar na lista: $out"
# 3) lista adulterada (hash não confere com o SHA256SUMS)
release "$T/r3" n 1; echo "$FPN" >> "$T/r3/revoked.txt"; out=$(run "$T/r3") && fail "lista adulterada deveria ser recusada"
# 4) a chave que assina não se revoga: sem rotação, lista da pinada (o) revogando o próprio o
release "$T/r4" o 0; out=$(run "$T/r4") || fail "lista da pinada deveria passar: $out"; printf '%s\n' "$out" | grep -qx "$FPO" && fail "a própria chave que assina não pode entrar na lista"
# 5) revogar barra o retorno: com a antiga já revogada, a release assinada por ela é recusada
echo "$FPO" > "$T/conf/revoked"; out=$(run "$T/r1") && fail "chave pinada revogada deveria ser recusada"; echo "$out" | grep -q "^sums:.*revogada" || fail "deveria dizer revogada: $out"
# 6) rotação para uma chave já revogada é recusada
echo "$FPN" > "$T/conf/revoked"; out=$(run "$T/r1") && fail "rotação para chave revogada deveria ser recusada"; echo "$out" | grep -q "^rot:" || fail "deveria recusar a rotação: $out"
echo "OK phase-u-revoke"
