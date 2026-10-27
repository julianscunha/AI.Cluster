# Desenho do console e da tela de carregamento do AI Cluster OS (busybox ash, TERM=linux).
# Carregado com `.` por aic-console e aic-splash. Nada aqui lê ou grava estado.
#
# Glifos de bloco e de caixa só saem com o VT em modo UTF-8; AIC_CONSOLE_ASCII=1 força o desenho
# em ASCII puro (serial, terminais sem a fonte).

ESC=$(printf '\033')

# Cores (SGR do VT linux). Só o cabeçalho e o rodapé têm fundo colorido; o resto é texto sobre preto.
C_RESET="${ESC}[0m"
C_BAR="${ESC}[1;37;44m"   # cabeçalho: branco forte sobre azul
C_FOOT="${ESC}[30;47m"    # rodapé: preto sobre cinza claro
C_LOGO="${ESC}[36m"       # logotipo e rótulos: ciano
C_NODE="${ESC}[1;36m"     # nós do logotipo: ciano forte
C_TEXT="${ESC}[37m"
C_VALUE="${ESC}[97m"
C_OK="${ESC}[92m"
C_WARN="${ESC}[93m"
C_BAD="${ESC}[91m"
C_HINT="${ESC}[2;37m"

if [ "${AIC_CONSOLE_ASCII:-0}" = 1 ]; then
	G_NODE='O' G_H='-' G_FILL='#' G_EMPTY='.' G_V='|' G_TL='+' G_TR='+' G_BL='+' G_BR='+'
else
	# Só glifos que a fonte padrão do VT tem (CP437): ■ (U+25A0), ─ (U+2500), █ e ░.
	G_NODE='■' G_H='─' G_FILL='█' G_EMPTY='░' G_V='│' G_TL='┌' G_TR='┐' G_BL='└' G_BR='┘'
fi

# Tamanho do terminal (linhas colunas), com piso para o desenho não quebrar.
term_size() {
	local size
	# TERM_DEV: o terminal a medir quando o stdin não é ele (o splash lê de /dev/null)
	size=$(stty ${TERM_DEV:+-F "$TERM_DEV"} size 2>/dev/null)
	LINES_N=${size%% *}
	COLS_N=${size##* }
	[ "${LINES_N:-0}" -ge 10 ] 2>/dev/null || LINES_N=25
	[ "${COLS_N:-0}" -ge 40 ] 2>/dev/null || COLS_N=80
}

# Margens que centram o bloco de 80x25 em telas maiores.
term_margins() {
	MX=0 MY=0
	[ "$COLS_N" -gt 80 ] && MX=$(((COLS_N - 80) / 2))
	[ "$LINES_N" -gt 25 ] && MY=$(((LINES_N - 25) / 2))
}

at() { printf '%s[%d;%dH' "$ESC" "$1" "$2"; } # linha coluna

# Entra no modo do console: UTF-8 ligado, sem quebra automática, cursor escondido.
tty_enter() {
	[ "${AIC_CONSOLE_ASCII:-0}" = 1 ] || printf '%s%%G' "$ESC"
	printf '%s[?7l%s[?25l' "$ESC" "$ESC"
}

# Uma faixa de largura total na linha $1, com o texto da esquerda $3 e o da direita $4 (só ASCII).
band() { # linha cor esquerda direita
	local pad=$((COLS_N - ${#3} - ${#4} - 2))
	[ "$pad" -lt 1 ] && pad=1
	at "$1" 1
	printf '%s %s%*s%s %s' "$2" "$3" "$pad" "" "$4" "$C_RESET"
}

# O logotipo: uma malha de sete nos, cinco linhas, a partir da linha $1 e da coluna $2.
logo() {
	local r=$1 c=$2 n="${C_NODE}${G_NODE}${C_LOGO}" h="${G_H}${G_H}${G_H}" bs='\'
	at "$r" "$c";       printf '%s   %s%s%s%s' "$C_LOGO" "$n" "$h" "$n" "$C_RESET"
	at $((r + 1)) "$c"; printf '%s  / %s / %s%s' "$C_LOGO" "$bs" "$bs" "$C_RESET"
	at $((r + 2)) "$c"; printf '%s %s%s%s%s%s%s' "$C_LOGO" "$n" "$h" "$n" "$h" "$n" "$C_RESET"
	at $((r + 3)) "$c"; printf '%s  %s / %s /%s' "$C_LOGO" "$bs" "$bs" "$C_RESET"
	at $((r + 4)) "$c"; printf '%s   %s%s%s%s' "$C_LOGO" "$n" "$h" "$n" "$C_RESET"
}

# Escreve o texto $3 (só ASCII) centrado na linha $1, com a cor $2.
center() { # linha cor texto
	local col=$(((COLS_N - ${#3}) / 2 + 1))
	[ "$col" -lt 1 ] && col=1
	at "$1" "$col"
	printf '%s%s%s' "$2" "$3" "$C_RESET"
}

# Repete o caractere $1, $2 vezes (em laço: o tr do busybox é por byte e quebra glifos UTF-8).
rep() {
	local i=0 out=
	while [ "$i" -lt "$2" ]; do
		out="$out$1"
		i=$((i + 1))
	done
	printf '%s' "$out"
}

# Uma moldura com título: linha, coluna, largura, altura (com as bordas) e o texto do título (ASCII).
box() {
	local r=$1 c=$2 w=$3 h=$4 title=$5 i
	at "$r" "$c"
	printf '%s%s%s %s%s%s %s%s%s' "$C_HINT" "$G_TL" "$G_H" "$C_LOGO" "$title" "$C_HINT" "$(rep "$G_H" $((w - ${#title} - 5)))" "$G_TR" "$C_RESET"
	i=1
	while [ "$i" -lt $((h - 1)) ]; do
		at $((r + i)) "$c"; printf '%s%s%s' "$C_HINT" "$G_V" "$C_RESET"
		at $((r + i)) $((c + w - 1)); printf '%s%s%s' "$C_HINT" "$G_V" "$C_RESET"
		i=$((i + 1))
	done
	at $((r + h - 1)) "$c"
	printf '%s%s%s%s%s' "$C_HINT" "$G_BL" "$(rep "$G_H" $((w - 2)))" "$G_BR" "$C_RESET"
}

# Barra de progresso de $1 por cento, com $2 colunas de largura.
progress() {
	local pct=$1 width=$2 filled i out=
	filled=$((pct * width / 100))
	i=0
	while [ "$i" -lt "$width" ]; do
		if [ "$i" -lt "$filled" ]; then out="$out$G_FILL"; else out="$out$G_EMPTY"; fi
		i=$((i + 1))
	done
	printf '%s%s%s' "$C_NODE" "$out" "$C_RESET"
}
