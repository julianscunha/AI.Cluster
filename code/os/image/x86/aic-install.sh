#!/bin/sh
# Ambiente de instalação do AI Cluster OS x86_64 (ADR-023), em RAM: é o /init do initramfs que
# build-installer.sh monta. Sobe a rede, espera a aprovação do controller, baixa a imagem e confere a
# assinatura do SHA256SUMS (chave pinada em release.pub), o SHA-256 e a versão (não aceita downgrade)
# antes de qualquer escrita; confere o disco aprovado com o anunciado, de novo logo antes de gravar;
# conta 10 s e só então pega a configuração e grava.
# Falha: diz o que fazer e para (não reinicia sozinho, para não entrar em laço de boot).
# AIC_INSTALL_LIB=1 carrega só as funções (é como code/tests/phase-u-x86-installer.sh as testa).
PATH=${PATH:+$PATH:}/bin:/sbin:/usr/bin:/usr/sbin
SYS=${AIC_SYS:-/sys}
DEV=${AIC_DEV:-/dev}
RUN=${AIC_RUN:-/run}
PUB=${AIC_PUB:-/etc/aic/release.pub}
MINVER=${AIC_MIN_VERSION_FILE:-/etc/aic/min-version}
IMGNAME=disk-x86.img.gz
FORCADO=${FORCADO:-false}   # true só se o operador aprovou com --force-removable (ver main)

fala() { printf '%s\n' "$*"; }
pare() { # falha: explica e fica parado; reiniciar sozinho viraria laço
	fala ""
	fala "ERRO: $*"
	fala "Nada foi apagado. Desligue o aparelho para tentar de novo."
	while :; do sleep 3600; done
}

conf() { # conf <chave>: aic.<chave>= na linha do kernel, senão <chave>= em /aic/installer.conf
	for w in $(cat /proc/cmdline 2>/dev/null); do
		case $w in aic.$1=*) echo "${w#aic.$1=}"; return 0;; esac
	done
	sed -n "s/^$1=//p" /aic/installer.conf 2>/dev/null | head -n 1
}

rede() { # DHCP em cada placa (a primeira que pega IP serve); mostra MAC e IP
	fala "Procurando rede..."
	placas=$(ls "$SYS"/class/net 2>/dev/null | grep -v '^lo$' | tr '
' ' ')
	[ -n "$placas" ] && fala "  placas de rede detectadas: $placas" 		|| fala "  nenhuma placa de rede detectada (driver ausente neste instalador?)"
	for rodada in 1 2 3; do
		for n in "$SYS"/class/net/*; do
			i=${n##*/}; [ "$i" = lo ] && continue
			ip link set "$i" up 2>/dev/null
			fala "  placa $i  MAC $(cat "$n/address" 2>/dev/null): pedindo IP..."
			udhcpc -i "$i" -n -q -t 4 -T 3 -s /etc/aic/udhcpc.sh >"$RUN/udhcpc-$i.log" 2>&1 || { tail -n 2 "$RUN/udhcpc-$i.log" | sed 's/^/    /'; continue; }
			fala "  placa $i  IP $(cat "$RUN/aic-ip-$i" 2>/dev/null)"
			return 0
		done
		sleep 5
	done
	return 1
}

discos() { # um disco por linha: nome, bytes, removível (1 = pendrive/USB/removível), modelo (só imprimível)
	for d in "$SYS"/block/*; do
		n=${d##*/}
		case $n in loop*|ram*|zram*|sr*|dm-*|md*|nbd*) continue;; esac
		case $n in *[!a-z0-9]*) continue;; esac
		s=$(cat "$d/size" 2>/dev/null) || continue
		[ "${s:-0}" -gt 0 ] 2>/dev/null || continue
		r=$(cat "$d/removable" 2>/dev/null)
		case $(readlink -f "$d") in */usb*) r=1;; esac
		m=$(cat "$d/device/model" 2>/dev/null | tr -cd '[:print:]' | tr -s ' ' | sed 's/ *$//')
		printf '%s %s %s %s\n' "$n" "$((s * 512))" "${r:-0}" "$m"
	done
}

seriais() { # nome, número de série e WWN de cada disco (lsblk); sem lsblk, não há o que conferir
	command -v lsblk >/dev/null 2>&1 || return 0
	lsblk -dno NAME,SERIAL,WWN 2>/dev/null | tr -cd '[:print:]\n' | tr -s ' '
}

dispositivo_de_boot() { # o que roda agora: partições montadas e root= na linha do kernel
	awk '$1 ~ /^\/dev\//{print $1}' /proc/mounts 2>/dev/null
	for w in $(cat /proc/cmdline 2>/dev/null); do case $w in root=/dev/*) echo "${w#root=}";; esac; done
}

tem_rotulo_aic() { # verdadeiro (= é o pendrive de boot) se alguma partição tem o rótulo AIC; sem blkid, recusa
	command -v blkid >/dev/null 2>&1 || return 0
	out=$(blkid 2>/dev/null) || return 0
	printf '%s\n' "$out" | grep -q "^/dev/$1p\{0,1\}[0-9]*: .*LABEL=\"AIC\""
}

disco_ok() { # disco_ok <nome>: existe no anúncio e não mudou; não é o pendrive, nem o sistema que roda
	n=$1
	l=$(awk -v n="$n" '$1==n' "$RUN/aic-discos.txt")
	[ -n "$l" ] || { echo "disco $n não estava no anúncio deste aparelho" >&2; return 1; }
	tem_rotulo_aic "$n" && { echo "disco $n é o pendrive de instalação (rótulo AIC) ou não dá para confirmar (blkid): não é apagado" >&2; return 1; }
	if [ "$(echo "$l" | awk '{print $3}')" != 0 ] && [ "$FORCADO" != true ]; then
		echo "disco $n é removível (pendrive ou USB): não é apagado sem --force-removable" >&2; return 1
	fi
	dispositivo_de_boot | grep -q "^/dev/$n\(p\{0,1\}[0-9]*\)\{0,1\}$" \
		&& { echo "disco $n tem o sistema que roda agora: não é apagado" >&2; return 1; }
	[ -e "$DEV/$n" ] || { echo "$DEV/$n não existe" >&2; return 1; }
	[ "$(discos | awk -v n="$n" '$1==n')" = "$l" ] || { echo "disco $n mudou desde o anúncio (tamanho ou modelo)" >&2; return 1; }
	sr=$(awk -v n="$n" '$1==n' "$RUN/aic-seriais.txt" 2>/dev/null)
	[ -z "$sr" ] || [ "$(seriais | awk -v n="$n" '$1==n')" = "$sr" ] || { echo "disco $n trocou de número de série ou WWN desde o anúncio" >&2; return 1; }
}

candidatos() { # discos que podem ser apagados, maior primeiro: internos; removíveis só se não houver interno
	for forca in false true; do
		out=
		for n in $(awk '{print $1}' "$RUN/aic-discos.txt"); do
			( FORCADO=$forca; disco_ok "$n" ) >/dev/null 2>&1 && out="$out$n "
		done
		[ -n "$out" ] || continue
		for n in $out; do awk -v n="$n" '$1==n' "$RUN/aic-discos.txt"; done | sort -k2,2nr
		return 0
	done
}

tecla() { # lê uma tecla em TECLA: cima, baixo, enter, 1-9, esc, outra ou vazio (nada no tempo); AIC_TECLAS é a fila de teste
	if [ -n "${AIC_TECLAS+x}" ]; then
		set -- $AIC_TECLAS; TECLA=${1:-}; [ $# -gt 0 ] && shift; AIC_TECLAS="$*"; return 0
	fi
	h=$(dd bs=1 count=1 2>/dev/null | od -An -tx1 | tr -d ' \n')
	case $h in
	'') TECLA= ;;
	0a|0d) TECLA=enter ;;
	1b)
		stty min 0 time 2 2>/dev/null
		q=$(dd bs=1 count=2 2>/dev/null | od -An -tx1 | tr -d ' \n')
		stty min 0 time 10 2>/dev/null
		case $q in 5b41|4f41) TECLA=cima;; 5b42|4f42) TECLA=baixo;; *) TECLA=esc;; esac ;;
	6b) TECLA=cima ;;
	6a) TECLA=baixo ;;
	3[1-9]) TECLA=${h#3} ;;
	*) TECLA=outra ;;
	esac
}

desenha_discos() { # desenha_discos <selecionado (1..n)>
	printf '\033[2J\033[H'
	fala "== AI.Cluster - em qual disco instalar? =="
	fala ""
	fala "Use as setas (cima/baixo) e Enter, ou digite o número do disco."
	fala ""
	printf '%s\n' "$CAND" | awk -v sel="$1" '{
		m=$4; for (i=5;i<=NF;i++) m=m " " $i
		t=($3==1)?"removível":"interno"
		printf "  %s %d) %-10s %6.0f GB  %-28s %s%s\n", (NR==sel)?">":" ", NR, $1, $2/1000000000, m, t, (NR==1)?"  [sugerido]":""
	}'
	fala ""
	fala "ATENÇÃO: o disco escolhido será APAGADO por completo (inclusive o Windows ou outro sistema que esteja nele)."
	fala "Nada é apagado agora: depois você aprova no controller (aic cluster install approve)."
	fala "Sem escolha em 60 s, o sugerido é anunciado ao controller."
}

escolhe_disco() { # define PREFERIDO (e AIC_PREFERRED_DISK, que o aic-wait anuncia); com mais de um candidato, pergunta na tela
	PREFERIDO=
	CAND=$(candidatos)
	[ -n "$CAND" ] || { fala "Nenhum disco deste computador pode ser apagado (veja os avisos do controller)."; return 0; }
	total=$(printf '%s\n' "$CAND" | wc -l)
	PREFERIDO=$(printf '%s\n' "$CAND" | head -n 1 | cut -d' ' -f1)
	if [ "$total" -gt 1 ] && { [ -t 0 ] || [ -n "${AIC_TECLAS+x}" ]; }; then
		[ "$total" -le 9 ] || { CAND=$(printf '%s\n' "$CAND" | head -n 9); total=9; }
		salva=$(stty -g 2>/dev/null)
		stty -icanon -echo min 0 time 10 2>/dev/null
		sel=1; ocioso=0
		desenha_discos "$sel"
		while [ "$ocioso" -lt 60 ]; do
			tecla
			case $TECLA in
			'') ocioso=$((ocioso + 1)); continue ;;
			cima) [ "$sel" -gt 1 ] && sel=$((sel - 1)) ;;
			baixo) [ "$sel" -lt "$total" ] && sel=$((sel + 1)) ;;
			[1-9]) [ "$TECLA" -le "$total" ] && { sel=$TECLA; TECLA=enter; } ;;
			esac
			ocioso=0
			[ "$TECLA" = enter ] && { PREFERIDO=$(printf '%s\n' "$CAND" | sed -n "${sel}p" | cut -d' ' -f1); break; }
			desenha_discos "$sel"
		done
		[ -n "$salva" ] && stty "$salva" 2>/dev/null
		printf '\033[2J\033[H'
	fi
	AIC_PREFERRED_DISK=$PREFERIDO; export AIC_PREFERRED_DISK
	fala "Disco para instalar: $PREFERIDO (anunciado ao controller; nada é apagado antes da sua aprovação)."
}

contagem() { # 10 s para desligar antes da escrita
	l=$(awk -v n="$1" '$1==n' "$RUN/aic-discos.txt")
	gb=$(echo "$l" | awk '{printf "%d GB", $2 / 1000000000}')
	modelo=$(echo "$l" | cut -d' ' -f4-)
	i=10
	while [ $i -gt 0 ]; do
		fala "apagando $1 $modelo $gb — desligue para cancelar ($i s)"
		sleep 1; i=$((i - 1))
	done
}

baixa() { # baixa <url> <arquivo>: até três tentativas
	t=1
	while [ $t -le 3 ]; do
		wget -q -O "$2" "$1" && return 0
		rm -f "$2"; t=$((t + 1)); sleep 5
	done
	return 1
}

versao_ok() { # versao_ok <pasta>: VERSION (já conferido no SHA256SUMS) é numérica e não é mais antiga que a mínima do build
	v=$(cat "$1/VERSION" 2>/dev/null)
	case $v in ''|*[!0-9.]*|.*|*.) echo "VERSION da imagem inválida: '$v'" >&2; return 1;; esac
	min=$(cat "$MINVER" 2>/dev/null)
	case $min in ''|*[!0-9.]*|.*|*.) echo "versão mínima ausente no instalador; refaça o build com AIC_RELEASE" >&2; return 1;; esac
	awk -v a="$v" -v b="$min" 'BEGIN { na = split(a, A, "."); nb = split(b, B, "."); n = na > nb ? na : nb
		for (i = 1; i <= n; i++) { x = A[i] + 0; y = B[i] + 0; if (x > y) exit 0; if (x < y) exit 1 }
		exit 0 }' || { echo "imagem $v é mais antiga que a mínima $min deste instalador (downgrade recusado)" >&2; return 1; }
}

confere() { # confere <pasta> <imagem>: assinatura do SHA256SUMS, SHA-256 da imagem, VERSION e versão mínima
	d=$1; img=$2
	base64 -d < "$d/SHA256SUMS.sig" > "$d/sig.bin" 2>/dev/null
	[ "$(wc -c < "$d/sig.bin")" = 64 ] || { echo "assinatura ilegível" >&2; return 1; }
	openssl pkeyutl -verify -pubin -inkey "$PUB" -rawin -in "$d/SHA256SUMS" -sigfile "$d/sig.bin" >/dev/null 2>&1 \
		|| { echo "assinatura do SHA256SUMS não confere com a chave pinada (release.pub)" >&2; return 1; }
	want=$(awk -v f="$img" '$2==f {print $1}' "$d/SHA256SUMS")
	[ -n "$want" ] || { echo "$img não consta no SHA256SUMS assinado" >&2; return 1; }
	have=$(sha256sum "$d/$img" | cut -d' ' -f1)
	[ "$want" = "$have" ] || { echo "SHA-256 de $img não confere (imagem corrompida ou trocada)" >&2; return 1; }
	wantv=$(awk '$2=="VERSION" {print $1}' "$d/SHA256SUMS")
	[ -n "$wantv" ] || { echo "VERSION não consta no SHA256SUMS assinado" >&2; return 1; }
	[ "$wantv" = "$(sha256sum "$d/VERSION" 2>/dev/null | cut -d' ' -f1)" ] || { echo "VERSION não confere com o SHA256SUMS" >&2; return 1; }
	versao_ok "$d"
}

preparar() { # sistemas de arquivos do ambiente e os módulos de rede, disco e firmware
	mount -t proc proc /proc 2>/dev/null
	mount -t sysfs sysfs /sys 2>/dev/null
	mount -t devtmpfs devtmpfs /dev 2>/dev/null
	mount -t tmpfs tmpfs /run 2>/dev/null
	mount -t tmpfs tmpfs /tmp 2>/dev/null
	mkdir -p /dev/pts "$RUN/img" /sys/firmware/efi/efivars
	mount -t efivarfs efivarfs /sys/firmware/efi/efivars 2>/dev/null
	while read -r m; do insmod "$m" 2>>"$RUN/modulos.log"; done < /etc/aic/modules
}

main() {
	fala ""
	fala "== AI.Cluster - instalador do AI Cluster OS (versão mínima $(cat /etc/aic/min-version 2>/dev/null)) =="
	fala ""
	fala "Carregando drivers..."
	preparar
	rede || {
		fala "Placas e o que o kernel disse delas:"; ls "$SYS"/class/net 2>/dev/null | sed 's/^/  /'
		dmesg 2>/dev/null | grep -iE 'eth|r8169|e1000|igb|igc|tg3|link|firmware' | tail -n 6 | sed 's/^/  /'
		pare "sem rede. Confira o cabo e o DHCP do roteador (ou a placa pode não ter driver aqui); depois desligue e ligue o aparelho."
	}
	CTRL=$(conf controller)
	IMGURL=$(conf image)
	[ -n "$CTRL" ] || pare "falta o endereço do controller (aic.controller= na linha do kernel, ou controller= em /aic/installer.conf)."
	[ -n "$IMGURL" ] || IMGURL="$CTRL/imagem"
	discos > "$RUN/aic-discos.txt"
	seriais > "$RUN/aic-seriais.txt"
	escolhe_disco
	fala ""
	fala "Aguardando a aprovação no controller $CTRL."
	fala "Confira a impressão digital acima com a do controller antes de aprovar."
	aic-wait "$CTRL" || pare "a espera pelo controller terminou sem aprovação (veja a mensagem acima)."

	TOKEN=$(sed -n 's/.*"token":"\([^"]*\)".*/\1/p' "$RUN/aic-install.json")
	DISCO=$(sed -n 's/.*"disk":"\([^"]*\)".*/\1/p' "$RUN/aic-install.json")
	FORCADO=$(sed -n 's/.*"forced":\(true\|false\).*/\1/p' "$RUN/aic-install.json")
	FORCADO=${FORCADO:-false}   # sem o campo, a aprovação não é de disco removível
	case $TOKEN in ''|*[!A-Za-z0-9._+/=-]*) pare "o controller aprovou sem um token válido; aprove de novo.";; esac
	case $DISCO in ''|*[!A-Za-z0-9]*) pare "o controller não disse qual disco apagar; aprove de novo.";; esac

	fala ""
	fala "Baixando a imagem, a versão e a assinatura do controller..."
	baixa "$IMGURL/SHA256SUMS" "$RUN/img/SHA256SUMS" && baixa "$IMGURL/SHA256SUMS.sig" "$RUN/img/SHA256SUMS.sig" \
		&& baixa "$IMGURL/VERSION" "$RUN/img/VERSION" && baixa "$IMGURL/$IMGNAME" "$RUN/img/$IMGNAME" \
		|| pare "não consegui baixar a imagem de $IMGURL. O token não foi usado: reinicie o aparelho para tentar de novo."
	confere "$RUN/img" "$IMGNAME" || pare "a imagem não passou na conferência (assinatura, SHA-256 ou versão). Não instale esta imagem; verifique o controller."
	fala "Imagem $(cat "$RUN/img/VERSION") conferida (assinatura, SHA-256 e versão)."

	fala ""
	cand=
	for n in $(awk '{print $1}' "$RUN/aic-discos.txt"); do disco_ok "$n" 2>/dev/null && cand="$cand $n"; done
	fala "Discos que podem ser apagados:${cand:- nenhum}"
	disco_ok "$DISCO" || pare "o disco aprovado ($DISCO) não pode ser apagado (veja a mensagem acima); aprove outro disco."

	contagem "$DISCO"
	disco_ok "$DISCO" || pare "o disco $DISCO mudou durante a contagem; nada foi apagado."

	fala "Pegando a configuração do worker no controller..."
	AIC_INSTALL_TOKEN="$TOKEN" aic-wait "$CTRL" --out "$RUN/worker.env" \
		|| pare "o controller não entregou a configuração (veja a mensagem acima). Nada foi apagado; aprove o aparelho de novo no controller."

	# de novo logo antes de gravar: a rede e o claim levam tempo, e o disco pode ter sido trocado nesse meio
	disco_ok "$DISCO" || pare "o disco $DISCO mudou depois da aprovação, antes da gravação. Nada foi apagado; aprove de novo (o token já foi usado)."
	fala "Gravando $DISCO..."
	install-disk.sh "$RUN/img/$IMGNAME" "$DEV/$DISCO" --yes --env "$RUN/worker.env" --efi-entry \
		|| pare "a gravação em $DEV/$DISCO falhou (veja a mensagem acima). O disco pode ter ficado pela metade: aprove o aparelho de novo no controller."
	sync
	fala ""
	fala "=================================================="
	fala "  INSTALADO."
	fala "  RETIRE O PENDRIVE e aperte Enter para reiniciar."
	fala "  O aparelho vai dar boot pelo disco."
	fala "=================================================="
	# Espera o operador (a tela fica; ele precisa ler). Sem teclado/tty: reinicia após 60 s.
	if [ -t 0 ]; then read -r _ || sleep 60; else sleep 60; fi
	reboot -f
	while :; do sleep 3600; done
}

[ "${AIC_INSTALL_LIB:-0}" = 1 ] || main
