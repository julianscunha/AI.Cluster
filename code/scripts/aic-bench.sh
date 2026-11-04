#!/bin/sh
# AI.Cluster - mede o desempenho do cluster pela API do gateway (Linux, macOS, WSL; só precisa de sh e curl).
# Faz N perguntas iguais com temperatura 0 e mostra, por rodada, os tokens por segundo gerando e lendo o
# prompt (medidos pelo próprio servidor), o tempo total e, se a decodificação especulativa estiver ligada,
# quantos tokens do modelo auxiliar foram aceitos. No fim, média, mínima e máxima.
#
#   AIC_API_KEY=SUA_CHAVE sh aic-bench.sh --url http://192.168.1.25:8080
#   sh aic-bench.sh --url 192.168.1.25 --runs 5 --tokens 128 --prompt "Conte até 10"
#
# Opções: --url ENDEREÇO  --key CHAVE (prefira AIC_API_KEY: --key fica no histórico)  --model ID
#         --prompt TEXTO  --tokens N (96)  --runs N (3)  --temperature T (0)  --timeout S (900)  --lang pt|en
# A chave está em /etc/ai-cluster/gateway.env no servidor (AI_CLUSTER_API_KEY). Para o Windows use
# aic-test-api.ps1. Compare sempre no mesmo prompt, mesmo modelo e com o cluster parado de outras cargas.
set -eu

URL=${AIC_URL:-}; KEY=${AIC_API_KEY:-}; MODEL=; PROMPT=; TOKENS=96; RUNS=3; TEMP=0; TIMEOUT=900
case "${LC_ALL:-${LANG:-}}" in pt*) LANG_=pt ;; *) LANG_=en ;; esac
t() { if [ "$LANG_" = pt ]; then printf '%s\n' "$1"; else printf '%s\n' "$2"; fi; }
die() { printf '\n  [x] %s\n\n' "$1" >&2; exit 1; }
need() { [ $# -ge 2 ] || die "$(t "faltou o valor de $1" "missing value for $1")"; }

while [ $# -gt 0 ]; do
	case "$1" in
	--url) need "$@"; URL=$2; shift 2 ;;
	--key) need "$@"; KEY=$2; shift 2 ;;
	--model) need "$@"; MODEL=$2; shift 2 ;;
	--prompt) need "$@"; PROMPT=$2; shift 2 ;;
	--tokens) need "$@"; TOKENS=$2; shift 2 ;;
	--runs) need "$@"; RUNS=$2; shift 2 ;;
	--temperature) need "$@"; TEMP=$2; shift 2 ;;
	--timeout) need "$@"; TIMEOUT=$2; shift 2 ;;
	--lang) need "$@"; LANG_=$2; shift 2 ;;
	-h | --help) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
	*) die "$(t "opção desconhecida: $1" "unknown option: $1")" ;;
	esac
done

command -v curl >/dev/null 2>&1 || die "$(t "falta o curl" "curl is missing")"
case "$TOKENS$RUNS$TIMEOUT" in *[!0-9]*) die "$(t "--tokens, --runs e --timeout são números inteiros" "--tokens, --runs and --timeout are whole numbers")" ;; esac
case "$TEMP" in *[!0-9.]* | "") die "$(t "--temperature é um número (ex.: 0 ou 0.7)" "--temperature is a number (e.g. 0 or 0.7)")" ;; esac
[ "$RUNS" -ge 1 ] && [ "$TOKENS" -ge 1 ] || die "$(t "--runs e --tokens precisam ser pelo menos 1" "--runs and --tokens must be at least 1")"
[ -n "$URL" ] || { printf '%s' "$(t "  Endereço do gateway [http://192.168.1.25:8080]: " "  Gateway address [http://192.168.1.25:8080]: ")"; read -r URL; URL=${URL:-http://192.168.1.25:8080}; }
case "$URL" in http://* | https://*) ;; *) URL=http://$URL ;; esac
case "${URL#*://}" in */* | *:*) ;; *) URL=$URL:8080 ;; esac
URL=${URL%/}
if [ -z "$KEY" ]; then
	printf '%s' "$(t "  Chave de API (fica escondida): " "  API key (hidden): ")"
	stty -echo 2>/dev/null || true; read -r KEY; stty echo 2>/dev/null || true; printf '\n'
	KEY=${KEY#AI_CLUSTER_API_KEY=}
fi
[ -n "$KEY" ] || die "$(t "informe --key ou AIC_API_KEY (em /etc/ai-cluster/gateway.env no servidor)" "give --key or AIC_API_KEY (in /etc/ai-cluster/gateway.env on the server)")"
[ -n "$PROMPT" ] || PROMPT=$(t "Escreva um parágrafo sobre redes de computadores." "Write a paragraph about computer networks.")

# A chave vai por stdin ao curl (-K -): não aparece na lista de processos.
api() { # método caminho [corpo]
	if [ $# -ge 3 ]; then
		printf 'header = "Authorization: Bearer %s"
' "$KEY" | curl -sS -m "$TIMEOUT" -K - -X "$1" "$URL$2" 			-H 'Content-Type: application/json' --data-binary "@$3" -w '
%{http_code}'
	else
		printf 'header = "Authorization: Bearer %s"
' "$KEY" | curl -sS -m "$TIMEOUT" -K - -X "$1" "$URL$2" -w '
%{http_code}'
	fi
}
# valor numérico de um campo do JSON (primeira ocorrência), 0 se faltar
num() { printf '%s' "$2" | tr ',{}' '\n\n\n' | sed -n "s/^ *\"$1\" *: *\([0-9.eE+-]*\) *\$/\1/p" | head -n 1 | sed 's/^$/0/'; }
status() { printf '%s' "$1" | tail -n 1; }
body() { printf '%s' "$1" | sed '$d'; }

printf '\n  AI.Cluster · %s  (%s)\n\n' "$(t "teste de desempenho" "performance test")" "$URL"
case "$URL" in http://*) printf '  [!] %s\n\n' "$(t "o endereço é http: a chave vai sem criptografia pela rede. Use só em rede de confiança." "the address is http: the key travels unencrypted over the network. Use only on a trusted network.")" ;; esac

R=$(api GET /health 2>&1) || die "$(t "o gateway não respondeu em $URL/health: confira o endereço, a porta e o firewall" "the gateway did not answer at $URL/health: check the address, port and firewall")"
[ "$(status "$R")" = 200 ] || die "$(t "/health respondeu $(status "$R")" "/health answered $(status "$R")")"
printf '  [ok] %s\n' "$(t "gateway no ar (/health)" "gateway is up (/health)")"
R=$(api GET /v1/models 2>&1) || die "$(t "falha ao listar os modelos" "failed to list the models")"
case "$(status "$R")" in 200) ;; 401 | 403) die "$(t "a chave de API foi recusada" "the API key was refused")" ;; *) die "/v1/models: HTTP $(status "$R")" ;; esac
[ -n "$MODEL" ] || MODEL=$(body "$R" | tr '}' '\n' | sed -n 's/.*"id":"\([^"]*\)","kind":"language-model".*/\1/p' | head -n 1)
[ -n "$MODEL" ] || die "$(t "nenhum modelo de linguagem ativo. No servidor: aic cluster model start <id>" "no language model is active. On the server: aic cluster model start <id>")"
printf '  [ok] %s: %s\n' "$(t "modelo" "model")" "$MODEL"
printf '  %s: %s\n\n' "$(t "Pergunta" "Question")" "$PROMPT"

# JSON do pedido: escapa \ e " e quebras de linha do prompt
esc=$(printf '%s' "$PROMPT" | sed 's/\\/\\\\/g; s/"/\\"/g' | tr '\n' ' ')
REQ="{\"model\":\"$MODEL\",\"max_tokens\":$TOKENS,\"temperature\":$TEMP,\"seed\":1,\"messages\":[{\"role\":\"user\",\"content\":\"$esc\"}]}"

SUM=$(mktemp); BODYF=$(mktemp)
# o curl do Windows não enxerga /tmp do Git Bash: converte o caminho quando há cygpath
BODYARG=$(cygpath -m "$BODYF" 2>/dev/null || printf '%s' "$BODYF")
trap 'rm -f "$SUM" "$BODYF"' EXIT
# O corpo vai por arquivo: um argumento com acento chegaria ao curl fora de UTF-8 no Windows (Git Bash).
printf '%s' "$REQ" > "$BODYF"
i=1
while [ "$i" -le "$RUNS" ]; do
	START=$(date +%s)
	R=$(api POST /v1/chat/completions "$BODYARG" 2>&1) || die "$(t "a conexão com o gateway caiu ou passou de ${TIMEOUT}s: $R" "the connection to the gateway dropped or exceeded ${TIMEOUT}s: $R")"
	END=$(date +%s)
	[ "$(status "$R")" = 200 ] || die "$(t "o chat falhou" "chat failed"): HTTP $(status "$R") $(body "$R" | head -c 200)"
	B=$(body "$R")
	gen=$(num predicted_per_second "$B"); pro=$(num prompt_per_second "$B")
	gn=$(num predicted_n "$B"); pn=$(num prompt_n "$B"); dn=$(num draft_n "$B"); da=$(num draft_n_accepted "$B")
	printf '  %s %s/%s: %s tok/s %s (%s %s), %s tok/s %s (%s %s), %ss' "$(t "Rodada" "Run")" "$i" "$RUNS" \
		"$(awk -v v="$gen" 'BEGIN{printf "%.2f", v}')" "$(t "gerando" "generating")" "$gn" "$(t "tokens" "tokens")" \
		"$(awk -v v="$pro" 'BEGIN{printf "%.2f", v}')" "$(t "lendo o prompt" "reading the prompt")" "$pn" "$(t "tokens" "tokens")" "$((END - START))"
	if [ "$(awk -v v="$dn" 'BEGIN{print (v > 0)}')" = 1 ]; then
		printf ', %s %s%%' "$(t "especulação aceita" "speculation accepted")" "$(awk -v a="$da" -v n="$dn" 'BEGIN{printf "%.0f", 100*a/n}')"
	fi
	printf '\n'
	printf '%s %s\n' "$gen" "$pro" >> "$SUM"
	i=$((i + 1))
done

echo
awk -v runs="$RUNS" -v lang="$LANG_" '
	{ g += $1; p += $2; if (NR == 1 || $1 < gmin) gmin = $1; if (NR == 1 || $1 > gmax) gmax = $1 }
	END {
		if (lang == "pt") printf "  Resumo (%d rodadas): geração média %.2f tok/s (mín %.2f, máx %.2f); prompt médio %.2f tok/s\n", runs, g/NR, gmin, gmax, p/NR
		else printf "  Summary (%d runs): mean generation %.2f tok/s (min %.2f, max %.2f); mean prompt %.2f tok/s\n", runs, g/NR, gmin, gmax, p/NR
	}' "$SUM"
t "  Dica: a 1ª rodada pode ser mais lenta (cache frio). Compare sempre com o mesmo prompt, --tokens e modelo." "  Tip: the 1st run can be slower (cold cache). Always compare with the same prompt, --tokens and model."
echo
