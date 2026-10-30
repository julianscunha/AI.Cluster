#!/usr/bin/env bash
# A caixa do convite e do fim da instalação (`box` em code/os/setup) fica alinhada: todas as linhas têm a
# mesma largura em CARACTERES, também com acento, símbolos de dois e três bytes e linhas longas.
# Regressão: a função contava o \n que o sed acrescenta e a borda saía 1 caractere maior que as linhas.
set -uo pipefail
HERE=$(cd "$(dirname "$0")/.." && pwd)
SETUP=${SETUP_OVERRIDE:-"$HERE/os/setup"}
T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
# clen e box vêm do próprio setup (do início de clen até o fim de box), sem cópia no teste.
python -X utf8 - "$SETUP" "$T/box.sh" <<'PY' || { echo "FALHA: não achei clen/box no setup"; exit 1; }
import io, sys
s = io.open(sys.argv[1], encoding='utf-8').read()
i = s.index('clen() {')
j = s.index('\n}\n', s.index('box() {')) + 3
io.open(sys.argv[2], 'w', encoding='utf-8', newline='\n').write('C=""; Z=""\n' + s[i:j] + '''
IP=172.27.0.2; PORT=8099; GCODE=PVGJXEPT
box "Worker (qualquer computador x86_64 ou ARM de 32 bits), rode:" "" \\
    "curl -fsSL http://$IP:$PORT/join-$GCODE | sudo sh" "" \\
    "Gateway noutro computador: sudo aic gateway install --server $IP:$PORT --code $GCODE" "" \\
    "Válido por 20 min. Linux · 60°C — ação ✔"
''')
PY
out=$(sh "$T/box.sh") || { echo "FALHA: box não rodou"; exit 1; }
widths=$(printf '%s\n' "$out" | python -X utf8 -c "import sys; print(sorted({len(l.rstrip(chr(10))) for l in sys.stdin if l.strip()}))")
[ "$(printf '%s' "$widths" | tr -cd ',' | wc -c)" = 0 ] || { echo "$out"; fail=1; echo "FALHA: larguras diferentes: $widths"; exit 1; }
echo "OK: todas as linhas da caixa têm a mesma largura $widths"
echo "=== caixa alinhada OK ==="
