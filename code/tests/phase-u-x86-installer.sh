#!/usr/bin/env bash
# Instalador x86 (ADR-023, etapa 3) sem hardware. Cinco partes:
#  A) sintaxe, e a ordem no instalador: a conferência do disco antes do install-disk.sh e gzip -t antes do dd;
#  B) escolha de disco com sysfs, lsblk e blkid de mentira: pendrive AIC sempre fora (inclusive com
#     --force-removable), removível só forçado, blkid com erro recusa, serial trocado recusa, modelo sanitizado;
#  C) conferência da imagem com chave de teste: assinatura, SHA-256, VERSION no SHA256SUMS e versão mínima
#     (downgrade recusado); o pacote íntegro passa;
#  D) initramfs montado (se build-installer.sh já rodou): binários, blkid, chave pinada, versão mínima, init.
# A gravação em disco (dd/GPT) não é exercitada: install-disk.sh só grava em disco inteiro. O countdown
# de 10 s também não. Nada toca em disco nem sobe QEMU.
set -uo pipefail
HERE=$(cd "$(dirname "$0")/.." && pwd)   # code/
X86=$HERE/os/image/x86
INST=$HERE/os/image/build/installer
T=${AIC_TMP:-/d/temp}/aic-installer-test.$$
teardown() { rm -rf "$T"; }
fail() { echo "FALHA: $1"; teardown; exit 1; }
trap teardown EXIT
mkdir -p "$T"
export AIC_INSTALL_LIB=1   # o aic-install.sh só carrega as funções

echo "=== A) sintaxe e ordem ==="
sh -n "$X86/aic-install.sh" || fail "aic-install.sh com erro de sintaxe"
sh -n "$X86/install-disk.sh" || fail "install-disk.sh com erro de sintaxe"
bash -n "$X86/build-installer.sh" || fail "build-installer.sh com erro de sintaxe"
last=$(grep -n 'disco_ok "\$DISCO"' "$X86/aic-install.sh" | tail -n 1 | cut -d: -f1)
inst=$(grep -n 'install-disk.sh "\$RUN' "$X86/aic-install.sh" | cut -d: -f1)
[ -n "$last" ] && [ -n "$inst" ] && [ "$last" -lt "$inst" ] || fail "a última conferência do disco não vem antes do install-disk.sh"
gz=$(grep -n 'gzip -t' "$X86/install-disk.sh" | head -n 1 | cut -d: -f1)
dd_=$(grep -n 'dd of=' "$X86/install-disk.sh" | head -n 1 | cut -d: -f1)
[ -n "$gz" ] && [ "$gz" -lt "$dd_" ] || fail "gzip -t não vem antes do dd em install-disk.sh"
# claim: a chave do túnel RPC nasce no aparelho; só o pedido (CSR) vai ao controller, e o certificado volta por --rpc-tls
grep -q 'openssl ecparam -name prime256v1 -genkey' "$X86/aic-install.sh" || fail "o instalador não gera a chave do túnel RPC no aparelho"
grep -q -- '--csr "\$RUN/rpc.csr" --rpc-out "\$RUN/rpc-tls"' "$X86/aic-install.sh" || fail "aic-wait sem --csr/--rpc-out"
grep -q -- '\$RPCARG --efi-entry' "$X86/aic-install.sh" || fail "install-disk.sh sem --rpc-tls"
! grep -n 'aic-wait.*worker\.key' "$X86/aic-install.sh" || fail "a chave do túnel não pode ir ao aic-wait"
echo "OK"

echo "=== B) disco: pendrive AIC sempre fora; removível só forçado; blkid e serial conferidos ==="
S=$T/sys; D=$T/dev; R=$T/run
mkdir -p "$S/block" "$D" "$R" "$T/bin"
mk() { mkdir -p "$S/block/$1/device"; echo "$2" > "$S/block/$1/size"; echo "$3" > "$S/block/$1/removable"; printf '%s' "$4" > "$S/block/$1/device/model"; : > "$D/$1"; }
mk sda 976773168 0 "SAMSUNG SSD 860"
mk sdb 30298112 1 "Pen Drive"
mk sdc 3907029168 0 "WD Blue"
mk nvme0n1 1953525168 0 "Samsung 980"
mk sdd 1000000 0 "$(printf 'Mod\001elo\tX')"
mk sr0 2048 1 "DVD"
cat > "$T/bin/blkid" <<'STUB'
#!/bin/sh
[ -n "${BLKID_FAIL:-}" ] && exit 1
echo '/dev/sdc1: LABEL="AIC" UUID="ABCD-1234" TYPE="vfat"'
echo '/dev/sdc2: LABEL="dados" TYPE="ext4"'
STUB
cat > "$T/bin/lsblk" <<'STUB'
#!/bin/sh
cat "$LSBLK_OUT"
STUB
chmod +x "$T/bin/blkid" "$T/bin/lsblk"
printf 'sda SER1 WWN1\nsdb SER2 WWN2\nsdc SER3 WWN3\nnvme0n1 SERN WWNN\n' > "$T/lsblk.ok"
export LSBLK_OUT=$T/lsblk.ok
# cada chamada roda numa subshell com o sysfs, o blkid e o lsblk de mentira
disco() { ( export PATH="$T/bin:$PATH" AIC_SYS="$S" AIC_DEV="$D" AIC_RUN="$R"; . "$X86/aic-install.sh"; "$@" ); }
disco discos > "$R/aic-discos.txt"
disco seriais > "$R/aic-seriais.txt"
grep -q '^sr0 ' "$R/aic-discos.txt" && fail "óptico (sr0) entrou no anúncio"

# 6) modelo com caractere de controle e tabulação: sai só imprimível, uma linha por disco
disco discos | grep -q "$(printf '\001')" && fail "caractere de controle no anúncio de disco"
l=$(disco discos | grep '^sdd ')
[ "$l" = "sdd 512000000 0 ModeloX" ] || fail "modelo do sdd não foi sanitizado: '$l'"

for n in sda nvme0n1; do disco disco_ok "$n" 2>/dev/null || fail "$n deveria poder ser apagado"; done
for n in sdb sdc sdz; do disco disco_ok "$n" 2>/dev/null && fail "$n não podia ser apagado sem force (removível, pendrive AIC ou fora do anúncio)"; done
out=$(disco disco_ok sdc 2>&1); echo "$out" | grep -q "rótulo AIC" || fail "mensagem do pendrive AIC não saiu: $out"
out=$(disco disco_ok sdb 2>&1); echo "$out" | grep -q "removível" || fail "mensagem do removível não saiu: $out"

# 2) --force-removable: o removível só sai com FORCADO=true; o pendrive AIC continua fora mesmo forçado
FORCADO=true disco disco_ok sdb 2>/dev/null || fail "removível forçado (--force-removable) foi recusado"
FORCADO=true disco disco_ok sdc 2>/dev/null && fail "pendrive AIC foi aceito com --force-removable"
FORCADO=false disco disco_ok sdb 2>/dev/null && fail "removível aceito sem force"

# 3) blkid com erro: sem certeza sobre o pendrive, recusa; sem blkid o script também recusa (command -v)
BLKID_FAIL=1 disco disco_ok sda 2>/dev/null && fail "blkid com erro não recusou o sda"
out=$(BLKID_FAIL=1 disco disco_ok sda 2>&1); echo "$out" | grep -q "blkid" || fail "mensagem do blkid com erro não saiu"
grep -q 'command -v blkid' "$X86/aic-install.sh" || fail "tem_rotulo_aic não confere se o blkid existe"

# 1) TOCTOU: o disco que muda de serial depois do anúncio é recusado (a conferência roda de novo antes de gravar)
printf 'sda SERX WWN1\nsdb SER2 WWN2\nsdc SER3 WWN3\nnvme0n1 SERN WWNN\n' > "$T/lsblk.trocado"
LSBLK_OUT=$T/lsblk.trocado disco disco_ok sda 2>/dev/null && fail "serial trocado desde o anúncio foi aceito"
out=$(LSBLK_OUT=$T/lsblk.trocado disco disco_ok sda 2>&1); echo "$out" | grep -q "número de série" || fail "mensagem do serial trocado não saiu: $out"

# tamanho e modelo trocados desde o anúncio (nome igual, disco diferente)
echo 1000 > "$S/block/sda/size"
disco disco_ok sda 2>/dev/null && fail "tamanho trocado desde o anúncio foi aceito"
echo 976773168 > "$S/block/sda/size"
printf '%s' "Outro Modelo" > "$S/block/sda/device/model"
disco disco_ok sda 2>/dev/null && fail "modelo trocado desde o anúncio foi aceito"
printf '%s' "SAMSUNG SSD 860" > "$S/block/sda/device/model"
disco disco_ok sda 2>/dev/null || fail "sda voltou ao anúncio original e foi recusado"

cand=$(for n in sda sdb sdc nvme0n1; do disco disco_ok "$n" 2>/dev/null && printf '%s ' "$n"; done)
[ "$cand" = "sda nvme0n1 " ] || fail "lista sem force: '$cand' (esperado 'sda nvme0n1 ')"
echo "OK: sem force sai 'sda nvme0n1'; com force o removível entra, o AIC não; blkid com erro e serial trocado recusam; modelo sanitizado"

# menu de disco na tela: ordem (maior primeiro), teclas (setas, dígito, Enter), tempo esgotado e candidato único
esc() { ( export AIC_TECLAS="$1"; disco eval 'escolhe_disco >/dev/null 2>&1; echo $PREFERIDO' ); }
[ "$(disco candidatos | awk '{print $1}' | tr '\n' ' ')" = "nvme0n1 sda sdd " ] || fail "candidatos fora de ordem (maior primeiro) ou com removível/AIC"
[ "$(esc 'enter')" = nvme0n1 ] || fail "Enter não escolheu o sugerido (maior interno)"
[ "$(esc 'baixo enter')" = sda ] || fail "seta para baixo + Enter não escolheu o 2o"
[ "$(esc 'baixo baixo cima enter')" = sda ] || fail "navegação com setas errada"
[ "$(esc '2')" = sda ] || fail "dígito 2 não escolheu o 2o disco"
[ "$(esc '9 outra enter')" = nvme0n1 ] || fail "dígito fora da lista deve ser ignorado"
[ "$(esc '')" = nvme0n1 ] || fail "sem tecla (60 s) deve seguir com o sugerido"
cp "$R/aic-discos.txt" "$T/discos.todos"
grep '^nvme0n1 ' "$T/discos.todos" > "$R/aic-discos.txt"
[ "$(esc 'baixo')" = nvme0n1 ] || fail "candidato único deve ser usado sem perguntar"
cp "$T/discos.todos" "$R/aic-discos.txt"
tela=$(export AIC_TECLAS=""; disco eval 'CAND=$(candidatos); desenha_discos 1')
echo "$tela" | grep -q "sugerido" || fail "a tela não marca o sugerido"
echo "$tela" | grep -q "APAGADO" || fail "a tela não avisa que o disco será APAGADO"
echo "OK: menu de disco (setas, dígito, Enter, 60 s e candidato único)"

echo "=== C) conferência da imagem: assinatura, SHA-256, VERSION e versão mínima ==="
K=$T/k; mkdir -p "$K"
(
  cd "$K" || exit 1
  openssl genpkey -algorithm ed25519 -out k.pem 2>/dev/null && openssl pkey -in k.pem -pubout -out pub.pem 2>/dev/null
  openssl genpkey -algorithm ed25519 -out outra.pem 2>/dev/null
  printf 'imagem de teste\n' | gzip -c > disk-x86.img.gz
) || fail "não consegui gerar a chave e a imagem de teste (openssl)"
cp "$K/pub.pem" "$T/pub.pem"
printf '1.0\n' > "$T/min-version"
# monta e assina SHA256SUMS com VERSION dentro de $K; $1 = versão a publicar
assinar() {
	(
		cd "$K" || exit 1
		printf '%s' "$1" > VERSION
		sha=$(sha256sum disk-x86.img.gz | cut -d' ' -f1)
		vsh=$(sha256sum VERSION | cut -d' ' -f1)
		printf '%s  disk-x86.img.gz\n%s  VERSION\n%s  outro.bin\n' "$sha" "$vsh" "$sha" > SHA256SUMS
		openssl pkeyutl -sign -inkey k.pem -rawin -in SHA256SUMS -out sig.bin && base64 -w0 < sig.bin > SHA256SUMS.sig
	)
}
assinar 1.1 || fail "não consegui assinar o pacote de teste"
pacote() { # pacote <pasta>: cópia íntegra da release de teste
	rm -rf "$T/$1"; mkdir -p "$T/$1"
	cp "$K/disk-x86.img.gz" "$K/SHA256SUMS" "$K/SHA256SUMS.sig" "$K/VERSION" "$T/$1/"
}
conferir() { # conferir <pasta> [arquivo-de-versão-mínima]: a conferência do aic-install.sh com a chave de teste
	( cd "$T" && export AIC_PUB=pub.pem AIC_MIN_VERSION_FILE="${2:-$T/min-version}" && . "$X86/aic-install.sh" && confere "$1" disk-x86.img.gz )
}
pacote ok
conferir ok 2>/dev/null || fail "pacote íntegro (versão 1.1, mínima 1.0) foi recusado"
echo "OK: pacote íntegro aceito"

recusa() { # recusa <pasta> <trecho esperado na mensagem> [arquivo-de-versão-mínima]
	out=$(conferir "$1" "${3:-}" 2>&1) && fail "$2: foi aceito"
	echo "$out" | grep -q "$2" || fail "$2: mensagem diferente: $out"
	echo "OK: recusado ($2)"
}
pacote imagem; printf 'x' >> "$T/imagem/disk-x86.img.gz"
recusa imagem "SHA-256"
pacote soma; echo "00  intruso.bin" >> "$T/soma/SHA256SUMS"
recusa soma "assinatura do SHA256SUMS"
pacote outra_chave
( cd "$K" && openssl pkeyutl -sign -inkey outra.pem -rawin -in SHA256SUMS -out outra.bin && base64 -w0 < outra.bin > "$T/outra_chave/SHA256SUMS.sig" )
recusa outra_chave "assinatura do SHA256SUMS"
pacote curta; head -c 32 "$K/sig.bin" | base64 -w0 > "$T/curta/SHA256SUMS.sig"
recusa curta "assinatura ilegível"
pacote versao_trocada; printf '1.2' > "$T/versao_trocada/VERSION"
recusa versao_trocada "VERSION não confere"
# 4) downgrade e replay: versão 0.9 assinada é mais antiga que a mínima 1.0 deste instalador
assinar 0.9
pacote antiga
recusa antiga "downgrade recusado"
assinar 1.1
# versão que não é número (ex.: 'dev') não passa
assinar dev
pacote dev
recusa dev "VERSION da imagem inválida"
assinar 1.1
# instalador sem versão mínima (build sem AIC_RELEASE) não aceita nada
pacote sem_minima
recusa sem_minima "versão mínima ausente" "$T/nao-existe"
# assinada pela chave certa, mas sem o nome da imagem nem VERSION: a assinatura passa e o nome não consta
( cd "$K" && printf '%s  outro.bin\n' "$(sha256sum disk-x86.img.gz | cut -d' ' -f1)" > sums.fora \
  && openssl pkeyutl -sign -inkey k.pem -rawin -in sums.fora -out s.bin && base64 -w0 < s.bin > sums.fora.sig ) \
  && pacote fora && cp "$K/sums.fora" "$T/fora/SHA256SUMS" && cp "$K/sums.fora.sig" "$T/fora/SHA256SUMS.sig" \
  || fail "não consegui montar o caso sem o nome da imagem"
recusa fora "não consta no SHA256SUMS"

echo "=== D) initramfs montado (build-installer.sh) ==="
if [ ! -f "$INST/initramfs.gz" ] || [ ! -s "$INST/vmlinuz" ]; then
	echo "PULADO: falta build/installer (rode bash code/os/image/x86/build-installer.sh)"
elif ! docker info >/dev/null 2>&1; then
	echo "PULADO: sem docker para listar o initramfs"
else
	WD=$(cd "$INST" && pwd -W 2>/dev/null || pwd)
	XD=$(cd "$X86" && pwd -W 2>/dev/null || pwd)
	OUT=$(MSYS_NO_PATHCONV=1 docker run --rm -v "$WD:/w:ro" -v "$XD:/src:ro" alpine:3.20 sh -c '
		mkdir /x && cd /x && busybox gzip -dc /w/initramfs.gz | busybox cpio -id 2>/dev/null
		for f in init bin/busybox bin/blkid bin/lsblk sbin/blockdev sbin/e2fsck usr/sbin/debugfs usr/sbin/resize2fs \
		         usr/sbin/efibootmgr usr/bin/openssl usr/sbin/sgdisk usr/bin/aic-wait usr/sbin/install-disk.sh \
		         etc/aic/release.pub etc/aic/min-version etc/aic/modules etc/aic/udhcpc.sh aic/installer.conf; do
			[ -e "$f" ] && echo "OK $f" || echo "FALTA $f"
		done
		cmp -s init /src/aic-install.sh && echo "OK init é o aic-install.sh" || echo "FALTA init igual ao aic-install.sh"
		cmp -s usr/sbin/install-disk.sh /src/install-disk.sh && echo "OK install-disk.sh é o do repositório" || echo "FALTA install-disk.sh igual ao do repositório"
		sh -n init && echo "OK sh -n init"
		openssl pkey -pubin -in etc/aic/release.pub -noout 2>/dev/null && echo "OK release.pub é chave pública"
		[ -n "$(cat etc/aic/min-version)" ] && echo "OK versão mínima $(cat etc/aic/min-version)"
		n=0; while read -r m; do if [ -f "/x$m" ]; then n=$((n + 1)); else echo "FALTA modulo $m"; fi; done < etc/aic/modules
		[ "$n" -gt 0 ] && echo "OK modulos $n"
	' 2>&1)
	echo "$OUT"
	echo "$OUT" | grep -q '^FALTA' && fail "initramfs incompleto"
	echo "$OUT" | grep -q '^OK modulos' || fail "nenhum módulo no initramfs"
	[ "$(echo "$OUT" | grep -c '^OK')" -ge 23 ] || fail "menos checagens OK do que o esperado"
	echo "OK: initramfs com os binários, blkid, a chave pinada, a versão mínima e o init"
fi
echo "OK: instalador x86 sem hardware passou"
