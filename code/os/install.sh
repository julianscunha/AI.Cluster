#!/bin/sh
# AI Cluster — instalador guiado. Pergunta o que este computador será, baixa da última release só o
# que esse papel precisa e instala. Não detecta nada: quem dita é o usuário (só confere se o
# processador informado é o da máquina, para não instalar um binário que não roda).
#
#   curl -fsSL https://github.com/julianscunha/Real.AI.Cluster/releases/latest/download/install.sh | sudo sh
#
# Sem perguntas (automação):
#   sudo sh install.sh --mode linux --role server|controller|gateway|worker --arch x86_64|armv7 \
#        [--kind fisico|vm|conteiner] [--server IP[:PORTA]] [--ip IP] [--models-dir DIR] [--version X.Y.Z] --yes
#
# Papéis: server = controller + gateway no mesmo computador; controller; gateway; worker.
# O worker entra pelo convite do servidor (`aic-setup invite` lá): --server aponta para ele.
# AIC_BASE troca a origem dos arquivos (espelho local ou teste).
set -eu

REPO=julianscunha/Real.AI.Cluster
MODE=""; ROLE=""; ARCH=""; KIND=""; SERVER=""; IP=""; MODELS=""; VERSION=""; YES=0

while [ $# -gt 0 ]; do
  case "$1" in
    --mode|--role|--arch|--kind|--server|--ip|--models-dir|--version)
      [ $# -ge 2 ] || { echo "$1 pede um valor" >&2; exit 2; }
      case "$1" in
        --mode) MODE=$2 ;; --role) ROLE=$2 ;; --arch) ARCH=$2 ;; --kind) KIND=$2 ;;
        --server) SERVER=$2 ;; --ip) IP=$2 ;; --models-dir) MODELS=$2 ;; --version) VERSION=$2 ;;
      esac
      shift 2 ;;
    --yes|-y) YES=1; shift ;;
    *) echo "argumento desconhecido: $1 (veja o cabeçalho do script)" >&2; exit 2 ;;
  esac
done

# ---------- apresentação (a mesma do aic-setup) ----------
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  B=$(printf '\033[1m'); D=$(printf '\033[2m'); G=$(printf '\033[32m'); R=$(printf '\033[31m')
  C=$(printf '\033[36m'); Y=$(printf '\033[33m'); Z=$(printf '\033[0m')
else
  B=""; D=""; G=""; R=""; C=""; Y=""; Z=""
fi
banner() {
  printf '\n%s  AI Cluster%s  %s%s%s\n' "$B$C" "$Z" "$D" "$1" "$Z"
  printf '  %s────────────────────────────────────────────────────────%s\n\n' "$D" "$Z"
}
die() { printf '\n  %s✘ %s%s\n\n' "$R" "$1" "$Z" >&2; exit 1; }
note() { printf '  %s%s%s\n' "$D" "$1" "$Z"; }
warn() { printf '  %s! %s%s\n' "$Y" "$1" "$Z"; }
ok() { printf '  %s✔%s %s\n' "$G" "$Z" "$1"; }

# ask <variável> <pergunta> <padrão> : lê do terminal (o script pode vir por pipe do curl).
TTY=/dev/tty
# [ -r /dev/tty ] vale mesmo sem terminal de controle (ssh -T, cron): abrir de verdade é o teste.
have_tty() { : <"$TTY" 2>/dev/null; }
ask() {
  have_tty || die "sem terminal para perguntar; use as opções (--role, --arch, ...) e --yes"
  printf '  %s%s%s [%s] ' "$B" "$2" "$Z" "$3" >&2
  read -r ans <"$TTY" || ans=""
  eval "$1=\${ans:-$3}"
}
# choose <variável> <padrão> : mostra o menu que já está na tela e lê o número.
choose() {
  have_tty || die "sem terminal para perguntar; use as opções (--role, --arch, ...) e --yes"
  printf '\n  Escolha %s[%s]%s: ' "$D" "$2" "$Z" >&2
  read -r ans <"$TTY" || ans=""
  eval "$1=\${ans:-$2}"
}

[ "$(id -u)" = 0 ] || die "rode como root: curl -fsSL .../install.sh | sudo sh"
[ "$(uname -s)" = Linux ] || die "este instalador é para Linux"
command -v curl >/dev/null || die "precisa do curl (apt install curl)"
command -v sha256sum >/dev/null || die "precisa do sha256sum (coreutils)"

banner "Bem-vindo"
if [ -z "$ROLE" ] || [ -z "$MODE" ] || [ -z "$ARCH" ]; then
  cat <<EOF
  O AI Cluster junta computadores comuns, novos ou velhos, para rodar modelos de IA que não cabem
  em um só. Ele tem 3 componentes:

   ${B}Controller${Z}  o cérebro. Registra os computadores, decide onde cada tarefa roda e guarda
               as chaves e os modelos. Um por cluster.
   ${B}Gateway${Z}     a porta de entrada. Recebe os pedidos de chat (API compatível com a da
               OpenAI) e divide o modelo entre os workers. Um por cluster. Só em x86_64.
   ${B}Worker${Z}      a força de trabalho. Empresta processador e memória. Quantos quiser.

EOF
fi

# ---------- 1. modo ----------
if [ -z "$MODE" ]; then
  cat <<EOF
  ${B}1. Como instalar neste computador?${Z}
     1) Neste Linux, como serviço  ${D}(rápido; divide o computador com o que já roda nele)${Z}
     2) Dedicar o computador ao cluster  ${D}(instala o AI Cluster OS; apaga o disco)${Z}
EOF
  choose a 1
  case "$a" in 1) MODE=linux ;; 2) MODE=dedicado ;; *) die "opção inválida: $a" ;; esac
fi
case "$MODE" in
  linux) ;;
  dedicado)
    cat <<EOF

  O modo dedicado grava o AI Cluster OS no disco: um sistema só para o cluster, imutável, com
  atualização A/B e volta automática se a atualização falhar. Desempenho previsível, sem outros
  serviços competindo.

  ${B}Hoje${Z} existem imagens do ${B}worker${Z} para ARM 32 bits (Orange Pi) e para PC x86_64 (UEFI).
  Controller e gateway dedicados ainda não estão nas imagens. Veja docs/guia/instalacao.md,
  seção "Instalação dedicada", e rode este instalador de novo em modo Linux para esses papéis.

EOF
    exit 0 ;;
  *) die "--mode deve ser linux ou dedicado" ;;
esac

# ---------- 2. papel ----------
if [ -z "$ROLE" ]; then
  cat <<EOF

  ${B}2. Este computador será:${Z}
     1) Controller  ${D}(o primeiro computador do cluster)${Z}
     2) Gateway     ${D}(a porta de entrada da API; precisa de um controller já instalado)${Z}
     3) Worker      ${D}(entra num cluster que já existe)${Z}
EOF
  choose a 1
  case "$a" in
    1)
      printf '
  %sO gateway vai neste mesmo computador, junto do controller?%s
' "$B" "$Z" >&2
      printf '     1) Sim, os dois aqui  %s(o mais simples)%s
     2) Não, o gateway fica em outro computador
' "$D" "$Z" >&2
      choose b 1
      case "$b" in 1) ROLE=server ;; 2) ROLE=controller ;; *) die "opção inválida: $b" ;; esac ;;
    2) ROLE=gateway ;;
    3) ROLE=worker ;;
    *) die "opção inválida: $a" ;;
  esac
fi
case "$ROLE" in server|controller|gateway|worker) ;; *) die "--role deve ser server, controller, gateway ou worker" ;; esac

# ---------- 3. hardware ----------
if [ -z "$ARCH" ]; then
  cat <<EOF

  ${B}3. Qual é o processador deste computador?${Z}
     1) PC ou servidor comum  ${D}(x86_64: Intel e AMD de 64 bits)${Z}
     2) ARM de 32 bits        ${D}(armv7: Orange Pi, Raspberry Pi antigo)${Z}
     3) ARM de 64 bits        ${D}(aarch64: Raspberry Pi 4/5, celulares, servidores ARM)${Z}
EOF
  choose a 1
  case "$a" in 1) ARCH=x86_64 ;; 2) ARCH=armv7 ;; 3) ARCH=aarch64 ;; *) die "opção inválida: $a" ;; esac
fi
if [ -z "$KIND" ]; then
  cat <<EOF

  ${B}Ele é:${Z}
     1) Físico   2) Máquina virtual   3) Contêiner
EOF
  choose a 1
  case "$a" in 1) KIND=fisico ;; 2) KIND=vm ;; 3) KIND=conteiner ;; *) die "opção inválida: $a" ;; esac
fi
case "$KIND" in fisico|vm|conteiner) ;; *) die "--kind deve ser fisico, vm ou conteiner" ;; esac

# Limites reais de hoje, ditos antes de baixar qualquer coisa.
case "$ARCH" in
  x86_64|armv7) ;;
  aarch64) die "ARM de 64 bits ainda não tem pacote. Planejado para a fase de celulares e ARM64 (docs/implementation/pendencias.md)." ;;
  *) die "--arch deve ser x86_64, armv7 ou aarch64" ;;
esac
case "$ROLE:$ARCH" in
  server:armv7|gateway:armv7) die "o gateway roda o llama-server, que só existe para x86_64. Em ARM de 32 bits instale o controller ou o worker." ;;
esac
# Sanidade, não detecção: um pacote de outro processador nem executa.
real=$(uname -m)
case "$ARCH:$real" in
  x86_64:x86_64|armv7:armv7l|armv7:armv7) ;;
  *) die "você informou $ARCH, mas este computador é $real. Escolha o processador certo." ;;
esac
[ "$KIND" = vm ] && warn "Máquina virtual: o desempenho depende do computador que a hospeda."
[ "$KIND" = conteiner ] && warn "Contêiner: sem systemd os serviços sobem em segundo plano e não voltam após reiniciar."

# ---------- 4. o que falta saber de cada papel ----------
need_server() {
  [ -n "$SERVER" ] && return 0
  printf '\n'
  note "No servidor, rode 'sudo aic-setup invite'. Ele mostra o endereço do convite."
  ask SERVER "Endereço do servidor (IP ou IP:porta do convite)" ""
  [ -n "$SERVER" ] || die "sem o endereço do servidor não dá para entrar no cluster"
}
case "$ROLE" in
  gateway|worker) need_server ;;
esac
CODE=""
if [ "$ROLE" = gateway ]; then
  if [ -z "${AIC_CODE:-}" ]; then
    note "O código aparece no convite do controller (sudo aic-setup invite), junto ao comando do gateway."
    ask CODE "Código do convite do gateway" ""
  else CODE=$AIC_CODE; fi
  [ -n "$CODE" ] || die "sem o código do convite o gateway não recebe as credenciais"
fi
case "$SERVER" in *[!A-Za-z0-9.:_-]*) die "endereço do servidor inválido" ;; esac

# ---------- resumo e confirmação ----------
case "$ROLE" in
  server) WHAT="controller + gateway" ;; controller) WHAT="controller" ;; gateway) WHAT="gateway" ;; worker) WHAT="worker" ;;
esac
printf '\n'
banner "Resumo"
note "Este computador será: $WHAT"
note "Processador: $ARCH · tipo: $KIND · instalação: neste Linux"
[ -n "$SERVER" ] && note "Servidor: $SERVER"
printf '\n'
if [ "$YES" != 1 ]; then
  have_tty || die "sem terminal para confirmar; use --yes"
  printf '  Continuar? [S/n] ' >&2
  read -r a <"$TTY" || a=n
  case "$a" in n|N|nao|não) die "cancelado" ;; esac
fi

# ---------- download da última release ----------
if [ -n "${AIC_BASE:-}" ]; then BASE=$AIC_BASE
elif [ -n "$VERSION" ]; then BASE="https://github.com/$REPO/releases/download/v$VERSION"
else BASE="https://github.com/$REPO/releases/latest/download"; fi

# Da release oficial só por HTTPS (sem downgrade por redirecionamento); AIC_BASE é para espelhos e testes.
PROTO=""; [ -n "${AIC_BASE:-}" ] || PROTO="--proto =https --tlsv1.2"
STAGE=$(mktemp -d); trap 'rm -rf "$STAGE"' EXIT
fetch() { # fetch <arquivo> : baixa para $STAGE e confere o SHA-256
  curl -fL $PROTO --progress-bar -o "$STAGE/$1" "$BASE/$1" || die "não consegui baixar $1 de $BASE"
  want=$(awk -v f="$1" '$2==f {print $1}' "$STAGE/SHA256SUMS")
  [ -n "$want" ] || die "$1 não consta no SHA256SUMS da release"
  have=$(sha256sum "$STAGE/$1" | cut -d' ' -f1)
  [ "$want" = "$have" ] || die "$1 não confere com o SHA256SUMS (corrompido ou adulterado)"
}
curl -fsSL $PROTO -o "$STAGE/SHA256SUMS" "$BASE/SHA256SUMS" || die "não consegui ler a release em $BASE"
curl -fsSL $PROTO -o "$STAGE/VERSION" "$BASE/VERSION" || die "a release não tem VERSION"
REL=$(cat "$STAGE/VERSION")
printf '\n'; banner "Baixando a versão $REL"

mkdir -p "$STAGE/aic"
need=""
case "$ROLE" in
  server) need="tools controller-linux-$ARCH gateway-linux-$ARCH" ;;
  controller) need="tools controller-linux-$ARCH" ;;
  gateway) need="tools gateway-linux-$ARCH" ;;
  worker) need="tools" ;;   # o worker vem do servidor, na mesma versão do cluster
esac
for f in $need; do
  fetch "$f.tar.gz"
  tar -C "$STAGE/aic" -xzf "$STAGE/$f.tar.gz"
  ok "$f"
done

# ---------- instalação ----------
export AIC_RELEASE_BASE="$BASE" AIC_RELEASE_VERSION="$REL" AIC_HW_KIND="$KIND" AIC_HW_ARCH="$ARCH"
set --
[ -n "$IP" ] && set -- "$@" --ip "$IP"
[ -n "$MODELS" ] && set -- "$@" --models-dir "$MODELS"
SETUP="$STAGE/aic/aic-setup"
rc=0
case "$ROLE" in
  server) "$SETUP" server --role both --yes "$@" || rc=$? ;;
  controller) "$SETUP" server --role controller --yes "$@" || rc=$? ;;
  gateway) "$SETUP" gateway --server "$SERVER" --code "$CODE" ${MODELS:+--models-dir "$MODELS"} --yes || rc=$? ;;
  worker) "$SETUP" worker --server "$SERVER" --yes || rc=$? ;;
esac
exit $rc
