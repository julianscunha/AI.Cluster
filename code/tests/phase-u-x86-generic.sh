#!/usr/bin/env bash
# Imagem x86_64 genérica (sem identidade): mk-disk.sh sem --controller/--token/--node-id não põe
# /worker.env na partição de dados; com as três opções põe, como antes. install-disk.sh --env grava o
# worker.env no p4 com debugfs (sem montar), e a função de gravação é a mesma do script, extraída dele.
# Roda numa cópia da x86/ em D:/temp com um rootfs falso de 1 MiB (o mk-disk só copia os bytes), então
# o build/ real não é tocado e nada pesado sobe. Sem loop device: install-disk.sh recusa o que não é
# disco inteiro, e isso não se contorna num contêiner. As funções são testadas no contêiner, não o dd/GPT.
set -uo pipefail
HERE=$(cd "$(dirname "$0")/.." && pwd)
SRC=$HERE/os/image/x86
T=/d/temp/aic-x86-generic.$$
teardown() { rm -rf "$T"; }
fail() { echo "FALHA: $1"; teardown; exit 1; }
trap teardown EXIT

mkdir -p "$T/os/image/x86" "$T/os/image/build" "$T/env"
TW=$(cd "$T" && pwd -W 2>/dev/null || echo "$T")   # caminho do Windows para o -v do Docker
cp "$SRC/mk-disk.sh" "$SRC/grub.cfg" "$T/os/image/x86/"
head -c 1048576 /dev/zero > "$T/os/image/build/rootfs-x86.ext4"   # o mk-disk só copia os bytes
printf 'AI_CLUSTER_CONTROLLER=https://controller.teste:50051\nAI_CLUSTER_JOIN_TOKEN=tok-teste\nAI_CLUSTER_NODE_ID=no-gravado\n' > "$T/env/novo.env"
printf 'AI_CLUSTER_NODE_ID=velho\n' > "$T/env/velho.env"
printf 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIteste op@pc
' > "$T/env/chaves"
# funções de gravação, extraídas do próprio script (sem cópia no teste)
sed -n '/^grava_env() {/,/^}/p; /^grava_ssh() {/,/^}/p; /^entrada_efi() {/,/^}/p' "$SRC/install-disk.sh" > "$T/env/funcoes.sh"
[ "$(grep -c '^grava_env() {\|^grava_ssh() {\|^entrada_efi() {' "$T/env/funcoes.sh")" = 3 ] || fail "não achei grava_env/grava_ssh/entrada_efi em install-disk.sh"

echo "=== A) imagem genérica: sem identidade, sem worker.env ==="
( cd "$T/os/image" && DISK_MIB=2200 bash x86/mk-disk.sh ) >"$T/mk-generic.log" 2>&1 || { cat "$T/mk-generic.log"; fail "mk-disk genérico falhou"; }
[ ! -e "$T/os/image/build/worker-x86.env" ] || fail "build/worker-x86.env existe na imagem genérica"
[ -f "$T/os/image/build/disk-x86.img" ] || fail "disk-x86.img não foi gerado"
echo "OK: mk-disk sem --controller gerou a imagem"

echo "=== B) imagem genérica: p4 sem worker.env; B2) grava com --env e confere ==="
MSYS_NO_PATHCONV=1 docker run --rm -v "$TW:/w" -w /w aic-x86-tools:1 sh -c '
set -e
cd /w/os/image/build
START=$(sgdisk -i 4 disk-x86.img | sed -n "s/^First sector: \([0-9]*\).*/\1/p")
LAST=$(sgdisk -i 4 disk-x86.img | sed -n "s/^Last sector: \([0-9]*\).*/\1/p")
dd if=disk-x86.img of=p4.img bs=512 skip=$START count=$((LAST - START + 1)) conv=sparse status=none
if debugfs -R "ls /" p4.img 2>/dev/null | grep -q worker.env; then echo "TEM_WORKER_ENV"; fi
echo "ls_ok"
. /w/env/funcoes.sh
grava_env /w/os/image/build/p4.img /w/env/novo.env && echo "gravou_1"
debugfs -R "stat worker.env" p4.img 2>/dev/null | grep -o "Mode: *[0-9]*"
debugfs -R "stat worker.env" p4.img 2>/dev/null | grep -o "User: *[0-9]*   Group: *[0-9]*"
debugfs -R "cat worker.env" p4.img 2>/dev/null | cmp /w/env/novo.env - && echo "conteudo_1_ok"
grava_env /w/os/image/build/p4.img /w/env/velho.env && echo "gravou_2"
debugfs -R "cat worker.env" p4.img 2>/dev/null | cmp /w/env/velho.env - && echo "regravacao_ok"
grava_ssh /w/os/image/build/p4.img /w/env/chaves && echo "ssh_gravou"
debugfs -R "stat ssh/authorized_keys" p4.img 2>/dev/null | grep -o "Mode: *[0-9]*" | sed "s/^/chave /"
debugfs -R "stat ssh" p4.img 2>/dev/null | grep -o "Mode: *[0-9]*" | sed "s/^/dir /"
grava_ssh /w/os/image/build/p4.img /w/env/chaves && echo "ssh_regravou"
' > "$T/b.log" 2>&1 || { cat "$T/b.log"; fail "passo B falhou"; }
cat "$T/b.log"
grep -q TEM_WORKER_ENV "$T/b.log" && fail "imagem genérica já tem worker.env"
grep -q "ls_ok" "$T/b.log" || fail "não listou o p4"
grep -q "gravou_1" "$T/b.log" || fail "grava_env falhou"
grep -q "Mode: *0600" "$T/b.log" || fail "worker.env não ficou com modo 0600"
grep -q "User: *0   Group: *0" "$T/b.log" || fail "worker.env não ficou com dono root"
grep -q "conteudo_1_ok" "$T/b.log" || fail "conteúdo gravado não confere"
grep -q "gravou_2" "$T/b.log" || fail "regravação falhou"
grep -q "regravacao_ok" "$T/b.log" || fail "regravação não substituiu o worker.env"
grep -q "ssh_gravou" "$T/b.log" && grep -q "ssh_regravou" "$T/b.log" || fail "grava_ssh falhou"
grep -q "chave Mode: *0600" "$T/b.log" && grep -q "dir Mode: *0700" "$T/b.log" || fail "ssh/authorized_keys sem 0600 ou ssh sem 0700"
echo "OK: p4 sem worker.env na genérica; --ssh grava ssh/authorized_keys 0600 em dir 0700; --env grava, com 0600 e root, e regrava"

echo "=== C) efibootmgr: ausente avisa e segue; presente sem NVRAM devolve erro ==="
# a imagem tem efibootmgr: a ausência é simulada com um PATH sem ele
MSYS_NO_PATHCONV=1 docker run --rm -v "$TW/env:/e" aic-x86-tools:1 sh -c '
PATH=/nao-existe; . /e/funcoes.sh
entrada_efi /dev/null' > "$T/c.log" 2>&1 || { cat "$T/c.log"; fail "entrada_efi sem efibootmgr devia seguir"; }
grep -q "efibootmgr não existe" "$T/c.log" || { cat "$T/c.log"; fail "não avisou da falta do efibootmgr"; }
MSYS_NO_PATHCONV=1 docker run --rm -v "$TW/env:/e" aic-x86-tools:1 sh -c '
. /e/funcoes.sh
entrada_efi /dev/null' > "$T/c2.log" 2>&1 && { cat "$T/c2.log"; fail "efibootmgr sem NVRAM deveria devolver erro"; }
echo "OK: sem efibootmgr avisa e segue; com falha do efibootmgr o erro volta ao install-disk (aviso)"

echo "=== D) imagem com identidade mantém /worker.env ==="
( cd "$T/os/image" && DISK_MIB=2200 bash x86/mk-disk.sh --controller https://controller.teste:50051 --token tok-teste --node-id no-1 ) >"$T/mk-id.log" 2>&1 || { cat "$T/mk-id.log"; fail "mk-disk com identidade falhou"; }
MSYS_NO_PATHCONV=1 docker run --rm -v "$TW:/w" -w /w aic-x86-tools:1 sh -c '
cd /w/os/image/build
START=$(sgdisk -i 4 disk-x86.img | sed -n "s/^First sector: \([0-9]*\).*/\1/p")
LAST=$(sgdisk -i 4 disk-x86.img | sed -n "s/^Last sector: \([0-9]*\).*/\1/p")
dd if=disk-x86.img of=p4.img bs=512 skip=$START count=$((LAST - START + 1)) conv=sparse status=none
debugfs -R "cat worker.env" p4.img 2>/dev/null | grep -q "^AI_CLUSTER_NODE_ID=no-1$" && echo "identidade_ok"
' > "$T/d.log" 2>&1 || { cat "$T/d.log"; fail "passo D falhou"; }
grep -q identidade_ok "$T/d.log" || { cat "$T/d.log"; fail "imagem com identidade não tem o worker.env"; }
echo "OK: com --controller/--token/--node-id o worker.env entra"

echo "=== E) opções incompletas recusadas antes de gerar nada ==="
( cd "$T/os/image" && bash x86/mk-disk.sh --controller https://controller.teste:50051 ) >"$T/e.log" 2>&1 && fail "identidade incompleta foi aceita"
grep -q "sem as três, a imagem é genérica" "$T/e.log" || fail "mensagem de identidade incompleta ausente"
echo "OK: identidade incompleta recusada"

echo "=== phase-u-x86-generic OK ==="
