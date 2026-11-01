#!/usr/bin/env bash
# Modo dedicado pelo navegador (ADR-023, etapa 4) sem hardware e sem QEMU. Quatro partes:
#  A) sintaxe: setup, mkusb.py, build-installer.sh e o servidor do convite (extraído do setup);
#  B) a pasta dedicado de teste (imagem, instalador e somas) assinada com chave de teste confere;
#  C) usb.zip (code/os/mkusb.py): arquivos, aic/grub.cfg com o IP e as duas portas, LEIAME nos dois
#     idiomas, conteúdo igual ao da pasta, e recusa de IP com caractere estranho e de arquivo faltando;
#  D) servidor do convite (o mesmo código do setup) serve /usb.zip, /imagem/<nome da lista> e mais nada:
#     o que foi baixado passa na conferência do aic-install.sh (assinatura, SHA-256, VERSION).
# Python: AIC_PY (padrão python3). Nada toca em disco nem sobe QEMU; o servidor é local (127.0.0.1).
set -uo pipefail
HERE=$(cd "$(dirname "$0")/.." && pwd)   # code/
X86=$HERE/os/image/x86
PY=${AIC_PY:-python3}
T=${AIC_TMP:-/d/temp}/aic-usb-test.$$
PORTA=${AIC_USB_PORT:-18099}
SRV_PID=""
teardown() { if [ -n "$SRV_PID" ]; then kill "$SRV_PID" 2>/dev/null; sleep 1; fi; rm -rf "$T"; }
fail() { echo "FALHA: $1"; teardown; exit 1; }
trap teardown EXIT
mkdir -p "$T"
# caminho que o Python nativo do Windows entende (no Linux, sem cygpath, é o próprio caminho)
win() { cygpath -m "$1" 2>/dev/null || printf '%s' "$1"; }
export AIC_INSTALL_LIB=1   # o aic-install.sh só carrega as funções

echo "=== A) sintaxe ==="
sh -n "$HERE/os/setup" || fail "setup com erro de sintaxe"
grep -q -- "-f \$d/aic/grub.cfg" "$X86/embed.cfg" || fail "o cfg embutido não percorre os discos atrás de /aic/grub.cfg"
grep -q -- "search --no-floppy --file /aic/grub.cfg" "$X86/embed.cfg" || fail "o cfg embutido não tem o fallback por search --file"
grep -q "menuentry" "$X86/embed.cfg" || fail "o cfg embutido não tem menu de falha (nada de prompt cru)"
grep -q "regexp" "$X86/build-installer.sh" || fail "falta o módulo regexp no GRUB (sem ele o (*) não expande)"
# o GRUB dá syntax error em bloco { } solto (||/&& { ...; }): só function/menuentry/if/for/while têm chaves
! grep -nE '(\|\||&&)[[:space:]]*\{' "$X86/embed.cfg" || fail "embed.cfg usa bloco { } solto (syntax error no GRUB)"
! grep -q -- "--label" "$X86/build-installer.sh" || fail "build-installer.sh voltou a procurar o pendrive por rótulo"
bash -n "$X86/build-installer.sh" || fail "build-installer.sh com erro de sintaxe"
"$PY" -c "import ast,sys; ast.parse(open(sys.argv[1],encoding='utf-8').read())" "$HERE/os/mkusb.py" || fail "mkusb.py com erro de sintaxe"
sed -n "/<<'PYSERVER'/,/^PYSERVER/p" "$HERE/os/setup" | sed '1d;$d' > "$T/invite.py"
[ -s "$T/invite.py" ] || fail "não achei o servidor do convite no setup"
"$PY" -c "import ast,sys; ast.parse(open(sys.argv[1],encoding='utf-8').read())" "$T/invite.py" || fail "servidor do convite com erro de sintaxe"

echo "=== B) pasta dedicado de teste, assinada com chave de teste ==="
D=$T/dedicado; mkdir -p "$D"
(
  cd "$T" || exit 1
  openssl genpkey -algorithm ed25519 -out k.pem 2>/dev/null && openssl pkey -in k.pem -pubout -out pub.pem 2>/dev/null
) || fail "não consegui gerar a chave de teste (openssl)"
(
  cd "$D" || exit 1
  printf 'imagem de teste\n' | gzip -c > disk-x86.img.gz
  printf '1.2\n' > VERSION
  printf 'EFI de teste\n' > BOOTX64.EFI
  printf 'kernel de teste\n' > vmlinuz
  printf 'initramfs de teste\n' | gzip -c > initramfs.gz
  cp "$HERE/os/mkusb.py" .
  for f in disk-x86.img.gz VERSION BOOTX64.EFI vmlinuz initramfs.gz mkusb.py; do
    printf '%s  %s\n' "$(sha256sum "$f" | cut -d' ' -f1)" "$f"
  done > SHA256SUMS
  openssl pkeyutl -sign -inkey "$(win "$T/k.pem")" -rawin -in SHA256SUMS -out sig.bin && base64 -w0 < sig.bin > SHA256SUMS.sig
) || fail "não consegui montar a pasta dedicado de teste"
base64 -d < "$D/SHA256SUMS.sig" > "$T/sig.bin"
( cd "$D" && openssl pkeyutl -verify -pubin -inkey "$(win "$T/pub.pem")" -rawin -in SHA256SUMS -sigfile "$(win "$T/sig.bin")" >/dev/null 2>&1 ) \
  || fail "a assinatura da pasta de teste não confere com a chave de teste"
( cd "$D" && sha256sum -c --quiet SHA256SUMS ) || fail "as somas da pasta de teste não conferem"

echo "=== C) usb.zip ==="
IP=192.168.1.10
"$PY" "$HERE/os/mkusb.py" "$D" "$T/usb.zip" "$IP" 50052 8099 || fail "mkusb.py falhou com a pasta completa"
[ -f "$T/usb.zip" ] && [ ! -f "$T/usb.zip.tmp" ] || fail "usb.zip não saiu (ou ficou um .tmp)"
"$PY" -I - "$(win "$T/usb.zip")" "$(win "$D")" "$IP" <<'PYCHECA' || fail "estrutura do usb.zip errada"
import sys, zipfile, pathlib
zip_path, pasta, ip = sys.argv[1], pathlib.Path(sys.argv[2]), sys.argv[3]
z = zipfile.ZipFile(zip_path)
nomes = set(z.namelist())
esperado = {"EFI/BOOT/BOOTX64.EFI", "aic/vmlinuz", "aic/initramfs.gz", "aic/grub.cfg", "aic/installer.conf", "LEIAME.txt"}
assert esperado <= nomes, ("faltam", esperado - nomes)
for n in ("EFI/BOOT/BOOTX64.EFI", "aic/vmlinuz", "aic/initramfs.gz"):
    origem = {"EFI/BOOT/BOOTX64.EFI": "BOOTX64.EFI", "aic/vmlinuz": "vmlinuz", "aic/initramfs.gz": "initramfs.gz"}[n]
    assert z.read(n) == (pasta / origem).read_bytes(), ("conteúdo diferente", n)
cfg = z.read("aic/grub.cfg").decode()
linha = [l.strip() for l in cfg.splitlines() if l.strip().startswith("linux ")]
assert len(linha) == 1, cfg
assert f"aic.controller=http://{ip}:50052" in linha[0], linha
assert f"aic.image=http://{ip}:8099/imagem" in linha[0], linha
assert "initrd /aic/initramfs.gz" in cfg, cfg
assert 'menuentry "Instalar AI Cluster OS"' in cfg and 'menuentry "Reiniciar' in cfg and "set timeout=5" in cfg, cfg
leia = z.read("LEIAME.txt").decode("utf-8")
assert "FAT32" in leia and "AIC" not in leia and "UEFI" in leia and "Secure Boot" in leia, leia
assert "Formate" in leia and "Format the pendrive" in leia, leia  # pt e en
assert ip in leia, leia
print("estrutura ok:", len(nomes), "arquivos")
PYCHECA
# o que o instalador lê de /proc/cmdline não pode vir com caractere que quebre a linha do grub
"$PY" "$HERE/os/mkusb.py" "$D" "$T/ruim.zip" "192.168.1.1\"; x" 50052 8099 >/dev/null 2>&1 && fail "IP com caractere estranho foi aceito"
"$PY" "$HERE/os/mkusb.py" "$D" "$T/ruim.zip" "$IP" 0 8099 >/dev/null 2>&1 && fail "porta 0 foi aceita"
rm -rf "$T/semvmlinuz"; mkdir -p "$T/semvmlinuz"; cp "$D"/* "$T/semvmlinuz/"; rm "$T/semvmlinuz/vmlinuz"
"$PY" "$HERE/os/mkusb.py" "$T/semvmlinuz" "$T/ruim.zip" "$IP" 50052 8099 >/dev/null 2>&1 && fail "zip saiu sem o kernel"
[ ! -f "$T/ruim.zip" ] && [ ! -f "$T/ruim.zip.tmp" ] || fail "zip parcial ficou no disco"

echo "=== D) servidor do convite: o que serve e o que é conferido ==="
W=$T/web; mkdir -p "$W"
cp "$T/usb.zip" "$W/usb.zip"
echo "ok" > "$W/join-TESTE"
(
  cd "$W" || exit 1
  AIC_CA_DIR="$T" AIC_INVITE_CODE=TESTE AIC_SIGNED_LOG="$T/signed" AIC_EVENTS_LOG="$T/events" AIC_TTL=120 \
    AIC_IMG_DIR="$(win "$D")" "$PY" "$(win "$T/invite.py")" 127.0.0.1 "$PORTA" >/dev/null 2>"$T/py.log" &
  echo $! > "$T/pid"
)
SRV_PID=$(cat "$T/pid")
base=http://127.0.0.1:$PORTA
for i in 1 2 3 4 5 6 7 8 9 10; do curl -s -o /dev/null "$base/usb.zip" && break; sleep 1; done
# --path-as-is: o curl não normaliza o ../ antes de mandar (é o caminho que chega ao servidor)
codigo() { curl -s --path-as-is -o /dev/null -w '%{http_code}' "$base$1"; }
[ "$(codigo /usb.zip)" = 200 ] || fail "/usb.zip não foi servido (veja $T/py.log)"
for nome in disk-x86.img.gz SHA256SUMS SHA256SUMS.sig VERSION; do
  [ "$(codigo /imagem/$nome)" = 200 ] || fail "/imagem/$nome não foi servido"
done
for caminho in /imagem/BOOTX64.EFI /imagem/vmlinuz /imagem/../../etc/passwd /imagem/%2e%2e/%2e%2e/etc/passwd /join-TESTE/../setup /SHA256SUMS; do
  [ "$(codigo "$caminho")" = 404 ] || fail "$caminho foi servido (esperado 404)"
done
# o que baixou tem de passar na conferência do instalador (a mesma função do aic-install.sh)
B=$T/baixado; mkdir -p "$B"
for nome in disk-x86.img.gz SHA256SUMS SHA256SUMS.sig VERSION; do curl -fsS -o "$B/$nome" "$base/imagem/$nome" || fail "download de $nome"; done
printf '1.0\n' > "$T/min-version"
( cd "$T" && export AIC_PUB="$(win "$T/pub.pem")" AIC_MIN_VERSION_FILE="$(win "$T/min-version")" && . "$X86/aic-install.sh" && confere "$(win "$B")" disk-x86.img.gz ) \
  || fail "a imagem servida não passou na conferência do aic-install.sh"
# e uma cópia adulterada não passa
printf 'x' >> "$B/disk-x86.img.gz"
( cd "$T" && export AIC_PUB="$(win "$T/pub.pem")" AIC_MIN_VERSION_FILE="$(win "$T/min-version")" && . "$X86/aic-install.sh" && confere "$(win "$B")" disk-x86.img.gz ) >/dev/null 2>&1 \
  && fail "imagem adulterada passou na conferência"
echo "usb.zip, imagem e somas servidos e conferidos"

echo "=== E) instalação da pasta dedicado (funções reais do setup, com AIC_SETUP_LIB=1) ==="
# ambiente de mentira: PREFIX, DATA e CONF em $T/ctl; a release em $T/rel; chown vira no-op (não somos root aqui)
ctl() { # ctl <pasta-da-release> <comando...>: roda uma função do setup nesse ambiente
  local rel=$1; shift
  ( AIC_SETUP_LIB=1 AIC_LANG=pt; . "$HERE/os/setup"
    PREFIX=$T/ctl/opt; DATA=$T/ctl/var; CONF=$T/ctl/etc; HERE=$rel
    chown() { :; }
    mkdir -p "$CONF"; cp "$T/pub.pem" "$CONF/release.pub"
    "$@" ) 2>&1
}
reset_ctl() { rm -rf "$T/ctl" "$T/rel"; mkdir -p "$T/ctl/var/dedicado" "$T/rel"; cp -r "$D" "$T/rel/dedicado"; echo lixo > "$T/ctl/var/dedicado/velho"; }
reset_ctl
ctl "$T/rel" instala_dedicado >/dev/null || fail "instala_dedicado falhou na pasta boa"
[ -f "$T/ctl/opt/dedicado/disk-x86.img.gz" ] || fail "a pasta boa não foi instalada em PREFIX/dedicado"
[ ! -e "$T/ctl/var/dedicado" ] || fail "cópia antiga em DATA (gravável pelo serviço) não foi removida"
ctl "$T/rel" dedicado_ok >/dev/null || fail "dedicado_ok recusou a pasta instalada e conferida"
echo "  pasta boa instalada; cópia antiga em DATA removida"
# links simbólicos: precisam de privilégio no Windows (MSYS copia em vez de linkar); onde não há, pula
LINK_OK=0; ln -s /etc/hostname "$T/probe-link" 2>/dev/null && [ -L "$T/probe-link" ] && LINK_OK=1
if [ "$LINK_OK" = 1 ]; then
  # a) link no lugar de um arquivo listado: recusa, e nada é instalado
  reset_ctl; echo segredo > "$T/fora-do-dedicado"
  rm -rf "$T/rel/dedicado/vmlinuz"; ln -s "$T/fora-do-dedicado" "$T/rel/dedicado/vmlinuz"
  out=$(ctl "$T/rel" instala_dedicado) || true
  echo "$out" | grep -q "não é um arquivo comum" || fail "link em arquivo listado não foi recusado: $out"
  [ ! -e "$T/ctl/opt/dedicado" ] || fail "pasta com link foi instalada"
  echo "  link recusado"
else
  echo "  (sem link simbólico neste sistema: caso a pulado aqui; roda em Linux)"
fi
# b) arquivo trocado (a assinatura do SHA256SUMS não muda, a soma muda): recusa
reset_ctl; echo troca >> "$T/rel/dedicado/vmlinuz"
out=$(ctl "$T/rel" instala_dedicado) || true
echo "$out" | grep -q "não confere" || fail "arquivo adulterado não foi recusado: $out"
[ ! -e "$T/ctl/opt/dedicado" ] || fail "pasta adulterada foi instalada"
echo "  adulteração recusada"
# c) arquivo fora da lista assinada não é copiado
reset_ctl; echo extra > "$T/rel/dedicado/extra.bin"
ctl "$T/rel" instala_dedicado >/dev/null || fail "instala_dedicado falhou com arquivo extra"
[ ! -e "$T/ctl/opt/dedicado/extra.bin" ] || fail "arquivo fora do SHA256SUMS foi copiado"
echo "  arquivo fora da lista não copiado"
# d) depois de instalada, uma cópia adulterada faz o convite recusar o pendrive
reset_ctl; ctl "$T/rel" instala_dedicado >/dev/null
echo troca >> "$T/ctl/opt/dedicado/vmlinuz"
ctl "$T/rel" dedicado_ok >/dev/null && fail "dedicado_ok aceitou a pasta instalada adulterada"
echo "  cópia instalada adulterada recusada pelo convite"

echo "=== F) pacote dedicado sem links (install.sh) ==="
# mesma regra que o install.sh aplica (cut -c1 da listagem tar -tvzf): link simbólico ou físico recusa
mkdir -p "$T/tarf/pacote"; echo x > "$T/tarf/pacote/a"
tar -C "$T/tarf/pacote" -czf "$T/sem-link.tgz" a 2>/dev/null
tem_link() { tar -tvzf "$1" | cut -c1 | grep -q '[lh]'; }
tem_link "$T/sem-link.tgz" && fail "pacote sem link foi recusado"
if [ "$LINK_OK" = 1 ]; then
  ln -s /etc/passwd "$T/tarf/pacote/l"
  tar -C "$T/tarf/pacote" -czf "$T/com-link.tgz" . 2>/dev/null
  tem_link "$T/com-link.tgz" || fail "pacote com link não foi pego"
fi
grep -q "tar -tvzf \"\$STAGE/dedicado.tar.gz\"" "$HERE/os/install.sh" || fail "install.sh não confere links antes de extrair"
echo "  pacote limpo passa (e o com link é recusado onde há link simbólico)"

echo "OK: phase-u-usb"
