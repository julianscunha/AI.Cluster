#!/bin/sh
# AI Cluster — instalador guiado. Pergunta o que este computador será, baixa da última release só o
# que esse papel precisa e instala. Não detecta nada: quem dita é o usuário (só confere se o
# processador informado é o da máquina, para não instalar um binário que não roda).
#
#   curl -fsSL https://github.com/julianscunha/Real.AI.Cluster/releases/latest/download/install.sh | sudo sh
#
# Sem perguntas (automação):
#   sudo sh install.sh --mode linux --role server|controller|gateway|worker --arch x86_64|armv7 \
#        [--kind fisico|vm|conteiner] [--server IP[:PORTA]] [--ip IP] [--models-dir DIR] [--version X.Y.Z] \
#        [--lang pt|en] --yes
#
# Papéis: server = controller + gateway no mesmo computador; controller; gateway; worker.
# O worker entra pelo convite do servidor (`aic-setup invite` lá): --server aponta para ele.
# AIC_BASE troca a origem dos arquivos (espelho local ou teste).
# Idioma: --lang pt|en (ou AIC_LANG=en no ambiente); padrão pt. Vale para o aic-setup e para o convite.
set -eu

REPO=julianscunha/Real.AI.Cluster
MODE=""; ROLE=""; ARCH=""; KIND=""; SERVER=""; IP=""; MODELS=""; VERSION=""; YES=0
LANG_AIC=${AIC_LANG:-pt}

# t <pt> <en>: a frase no idioma escolhido (--lang ou AIC_LANG; padrão pt).
t() { if [ "$LANG_AIC" = en ]; then printf '%s' "$2"; else printf '%s' "$1"; fi; }

while [ $# -gt 0 ]; do
  case "$1" in
    --mode|--role|--arch|--kind|--server|--ip|--models-dir|--version|--lang)
      [ $# -ge 2 ] || { echo "$(t "$1 pede um valor" "$1 needs a value")" >&2; exit 2; }
      case "$1" in
        --mode) MODE=$2 ;; --role) ROLE=$2 ;; --arch) ARCH=$2 ;; --kind) KIND=$2 ;;
        --server) SERVER=$2 ;; --ip) IP=$2 ;; --models-dir) MODELS=$2 ;; --version) VERSION=$2 ;;
        --lang) LANG_AIC=$2 ;;
      esac
      shift 2 ;;
    --yes|-y) YES=1; shift ;;
    *) echo "$(t "argumento desconhecido: $1 (veja o cabeçalho do script)" "unknown argument: $1 (see the script header)")" >&2; exit 2 ;;
  esac
done
case "$LANG_AIC" in pt|en) ;; *) echo "idioma inválido: $LANG_AIC (use pt ou en) / invalid language: $LANG_AIC (use pt or en)" >&2; exit 2 ;; esac
export AIC_LANG=$LANG_AIC

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
  have_tty || die "$(t 'sem terminal para perguntar; use as opções (--role, --arch, ...) e --yes' 'no terminal to ask on; use the options (--role, --arch, ...) and --yes')"
  printf '  %s%s%s [%s] ' "$B" "$2" "$Z" "$3" >&2
  read -r ans <"$TTY" || ans=""
  eval "$1=\${ans:-$3}"
}
# choose <variável> <padrão> : mostra o menu que já está na tela e lê o número.
choose() {
  have_tty || die "$(t 'sem terminal para perguntar; use as opções (--role, --arch, ...) e --yes' 'no terminal to ask on; use the options (--role, --arch, ...) and --yes')"
  printf '\n  %s %s[%s]%s: ' "$(t Escolha Choose)" "$D" "$2" "$Z" >&2
  read -r ans <"$TTY" || ans=""
  eval "$1=\${ans:-$2}"
}

[ "$(id -u)" = 0 ] || die "$(t 'rode como root: curl -fsSL .../install.sh | sudo sh' 'run as root: curl -fsSL .../install.sh | sudo sh')"
[ "$(uname -s)" = Linux ] || die "$(t 'este instalador é para Linux' 'this installer is for Linux')"
command -v curl >/dev/null || die "$(t 'precisa do curl (apt install curl)' 'needs curl (apt install curl)')"
command -v sha256sum >/dev/null || die "$(t 'precisa do sha256sum (coreutils)' 'needs sha256sum (coreutils)')"

banner "$(t Bem-vindo Welcome)"
if [ -z "$ROLE" ] || [ -z "$MODE" ] || [ -z "$ARCH" ]; then
  if [ "$LANG_AIC" = en ]; then
    cat <<EOF
  The AI Cluster joins ordinary computers, new or old, to run AI models that do not fit in just one.
  It has 3 components:

   ${B}Controller${Z}  the brain. Registers the computers, decides where each task runs and keeps the
               keys and the models. One per cluster.
   ${B}Gateway${Z}     the front door. Receives the chat requests (an API compatible with OpenAI's) and
               splits the model among the workers. One per cluster. Only on x86_64.
   ${B}Worker${Z}      the workforce. Lends processor and memory. As many as you want.

EOF
  else
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
fi

# ---------- 1. modo ----------
if [ -z "$MODE" ]; then
  if [ "$LANG_AIC" = en ]; then
    cat <<EOF
  ${B}1. How do you want to install on this computer?${Z}
     1) On this Linux, as a service  ${D}(quick; shares the computer with what already runs on it)${Z}
     2) Dedicate the computer to the cluster  ${D}(installs the AI Cluster OS; erases the disk)${Z}
EOF
  else
    cat <<EOF
  ${B}1. Como instalar neste computador?${Z}
     1) Neste Linux, como serviço  ${D}(rápido; divide o computador com o que já roda nele)${Z}
     2) Dedicar o computador ao cluster  ${D}(instala o AI Cluster OS; apaga o disco)${Z}
EOF
  fi
  choose a 1
  case "$a" in 1) MODE=linux ;; 2) MODE=dedicado ;; *) die "$(t "opção inválida: $a" "invalid option: $a")" ;; esac
fi
case "$MODE" in
  linux) ;;
  dedicado|dedicated)
    if [ "$LANG_AIC" = en ]; then
      cat <<EOF

  Dedicated mode writes the AI Cluster OS to the disk: a system only for the cluster, immutable, with
  A/B updates and an automatic rollback if an update fails. Predictable performance, with no other
  services competing.

  ${B}Today${Z} there are images of the ${B}worker${Z} for 32-bit ARM (Orange Pi) and for x86_64 PC (UEFI).
  Dedicated controller and gateway are not in the images yet. See docs/guia/instalacao.md (in
  Portuguese), section "Instalação dedicada", and run this installer again in Linux mode for those roles.

EOF
    else
      cat <<EOF

  O modo dedicado grava o AI Cluster OS no disco: um sistema só para o cluster, imutável, com
  atualização A/B e volta automática se a atualização falhar. Desempenho previsível, sem outros
  serviços competindo.

  ${B}Hoje${Z} existem imagens do ${B}worker${Z} para ARM 32 bits (Orange Pi) e para PC x86_64 (UEFI).
  Controller e gateway dedicados ainda não estão nas imagens. Veja docs/guia/instalacao.md,
  seção "Instalação dedicada", e rode este instalador de novo em modo Linux para esses papéis.

EOF
    fi
    exit 0 ;;
  *) die "$(t '--mode deve ser linux ou dedicado' '--mode must be linux or dedicated')" ;;
esac

# ---------- 2. papel ----------
if [ -z "$ROLE" ]; then
  if [ "$LANG_AIC" = en ]; then
    cat <<EOF

  ${B}2. This computer will be:${Z}
     1) Controller  ${D}(the first computer of the cluster)${Z}
     2) Gateway     ${D}(the front door of the API; needs a controller already installed)${Z}
     3) Worker      ${D}(joins a cluster that already exists)${Z}
EOF
  else
    cat <<EOF

  ${B}2. Este computador será:${Z}
     1) Controller  ${D}(o primeiro computador do cluster)${Z}
     2) Gateway     ${D}(a porta de entrada da API; precisa de um controller já instalado)${Z}
     3) Worker      ${D}(entra num cluster que já existe)${Z}
EOF
  fi
  choose a 1
  case "$a" in
    1)
      if [ "$LANG_AIC" = en ]; then
        printf '
  %sWill the gateway run on this same computer, next to the controller?%s
' "$B" "$Z" >&2
        printf '     1) Yes, both here  %s(the simplest)%s
     2) No, the gateway runs on another computer
' "$D" "$Z" >&2
      else
        printf '
  %sO gateway vai neste mesmo computador, junto do controller?%s
' "$B" "$Z" >&2
        printf '     1) Sim, os dois aqui  %s(o mais simples)%s
     2) Não, o gateway fica em outro computador
' "$D" "$Z" >&2
      fi
      choose b 1
      case "$b" in 1) ROLE=server ;; 2) ROLE=controller ;; *) die "$(t "opção inválida: $b" "invalid option: $b")" ;; esac ;;
    2) ROLE=gateway ;;
    3) ROLE=worker ;;
    *) die "$(t "opção inválida: $a" "invalid option: $a")" ;;
  esac
fi
case "$ROLE" in server|controller|gateway|worker) ;; *) die "$(t '--role deve ser server, controller, gateway ou worker' '--role must be server, controller, gateway or worker')" ;; esac

# ---------- 3. hardware ----------
if [ -z "$ARCH" ]; then
  if [ "$LANG_AIC" = en ]; then
    cat <<EOF

  ${B}3. What is the processor of this computer?${Z}
     1) Common PC or server  ${D}(x86_64: 64-bit Intel and AMD)${Z}
     2) 32-bit ARM          ${D}(armv7: Orange Pi, old Raspberry Pi)${Z}
     3) 64-bit ARM          ${D}(aarch64: Raspberry Pi 4/5, phones, ARM servers)${Z}
EOF
  else
    cat <<EOF

  ${B}3. Qual é o processador deste computador?${Z}
     1) PC ou servidor comum  ${D}(x86_64: Intel e AMD de 64 bits)${Z}
     2) ARM de 32 bits        ${D}(armv7: Orange Pi, Raspberry Pi antigo)${Z}
     3) ARM de 64 bits        ${D}(aarch64: Raspberry Pi 4/5, celulares, servidores ARM)${Z}
EOF
  fi
  choose a 1
  case "$a" in 1) ARCH=x86_64 ;; 2) ARCH=armv7 ;; 3) ARCH=aarch64 ;; *) die "$(t "opção inválida: $a" "invalid option: $a")" ;; esac
fi
if [ -z "$KIND" ]; then
  if [ "$LANG_AIC" = en ]; then
    cat <<EOF

  ${B}It is:${Z}
     1) Physical   2) Virtual machine   3) Container
EOF
  else
    cat <<EOF

  ${B}Ele é:${Z}
     1) Físico   2) Máquina virtual   3) Contêiner
EOF
  fi
  choose a 1
  case "$a" in 1) KIND=fisico ;; 2) KIND=vm ;; 3) KIND=conteiner ;; *) die "$(t "opção inválida: $a" "invalid option: $a")" ;; esac
fi
case "$KIND" in
  fisico|physical) KIND=fisico ;;
  vm) ;;
  conteiner|container) KIND=conteiner ;;
  *) die "$(t '--kind deve ser fisico, vm ou conteiner' '--kind must be physical (fisico), vm or container (conteiner)')" ;;
esac

# Limites reais de hoje, ditos antes de baixar qualquer coisa.
case "$ARCH" in
  x86_64|armv7) ;;
  aarch64) die "$(t 'ARM de 64 bits ainda não tem pacote. Planejado para a fase de celulares e ARM64 (docs/implementation/pendencias.md).' '64-bit ARM has no package yet. Planned for the phones and ARM64 phase (docs/implementation/pendencias.md).')" ;;
  *) die "$(t '--arch deve ser x86_64, armv7 ou aarch64' '--arch must be x86_64, armv7 or aarch64')" ;;
esac
case "$ROLE:$ARCH" in
  server:armv7|gateway:armv7) die "$(t 'o gateway roda o llama-server, que só existe para x86_64. Em ARM de 32 bits instale o controller ou o worker.' 'the gateway runs llama-server, which exists only for x86_64. On 32-bit ARM install the controller or the worker.')" ;;
esac
# Sanidade, não detecção: um pacote de outro processador nem executa.
real=$(uname -m)
case "$ARCH:$real" in
  x86_64:x86_64|armv7:armv7l|armv7:armv7) ;;
  *) die "$(t "você informou $ARCH, mas este computador é $real. Escolha o processador certo." "you chose $ARCH, but this computer is $real. Choose the right processor.")" ;;
esac
[ "$KIND" = vm ] && warn "$(t 'Máquina virtual: o desempenho depende do computador que a hospeda.' 'Virtual machine: performance depends on the computer hosting it.')"
[ "$KIND" = conteiner ] && warn "$(t 'Contêiner: sem systemd os serviços sobem em segundo plano e não voltam após reiniciar.' 'Container: without systemd the services run in the background and do not come back after a restart.')"

# ---------- 4. o que falta saber de cada papel ----------
need_server() {
  [ -n "$SERVER" ] && return 0
  printf '\n'
  note "$(t "No servidor, rode 'sudo aic-setup invite'. Ele mostra o endereço do convite." "On the server, run 'sudo aic-setup invite'. It shows the invitation address.")"
  ask SERVER "$(t 'Endereço do servidor (IP ou IP:porta do convite)' 'Server address (IP or IP:port of the invitation)')" ""
  [ -n "$SERVER" ] || die "$(t 'sem o endereço do servidor não dá para entrar no cluster' 'without the server address you cannot join the cluster')"
}
case "$ROLE" in
  gateway|worker) need_server ;;
esac
CODE=""
if [ "$ROLE" = gateway ]; then
  if [ -z "${AIC_CODE:-}" ]; then
    note "$(t 'O código aparece no convite do controller (sudo aic-setup invite), junto ao comando do gateway.' 'The code appears in the controller invitation (sudo aic-setup invite), next to the gateway command.')"
    ask CODE "$(t 'Código do convite do gateway' 'Gateway invitation code')" ""
  else CODE=$AIC_CODE; fi
  [ -n "$CODE" ] || die "$(t 'sem o código do convite o gateway não recebe as credenciais' 'without the invitation code the gateway does not receive its credentials')"
fi
case "$SERVER" in *[!A-Za-z0-9.:_-]*) die "$(t 'endereço do servidor inválido' 'invalid server address')" ;; esac

# ---------- resumo e confirmação ----------
case "$ROLE" in
  server) WHAT="controller + gateway" ;; controller) WHAT="controller" ;; gateway) WHAT="gateway" ;; worker) WHAT="worker" ;;
esac
case "$KIND" in fisico) KS=$(t fisico physical) ;; vm) KS=$(t vm 'virtual machine') ;; *) KS=$(t conteiner container) ;; esac
printf '\n'
banner "$(t Resumo Summary)"
note "$(t "Este computador será: $WHAT" "This computer will be: $WHAT")"
note "$(t "Processador: $ARCH · tipo: $KS · instalação: neste Linux" "Processor: $ARCH · type: $KS · installation: on this Linux")"
[ -n "$SERVER" ] && note "$(t "Servidor: $SERVER" "Server: $SERVER")"
printf '\n'
if [ "$YES" != 1 ]; then
  have_tty || die "$(t 'sem terminal para confirmar; use --yes' 'no terminal to confirm; use --yes')"
  if [ "$LANG_AIC" = en ]; then
    printf '  Continue? [Y/n] ' >&2
    read -r a <"$TTY" || a=n
    case "$a" in n|N|no|No|NO) die "cancelled" ;; esac
  else
    printf '  Continuar? [S/n] ' >&2
    read -r a <"$TTY" || a=n
    case "$a" in n|N|nao|não) die "cancelado" ;; esac
  fi
fi

# ---------- download da última release ----------
if [ -n "${AIC_BASE:-}" ]; then BASE=$AIC_BASE
elif [ -n "$VERSION" ]; then BASE="https://github.com/$REPO/releases/download/v$VERSION"
else BASE="https://github.com/$REPO/releases/latest/download"; fi

# Da release oficial só por HTTPS (sem downgrade por redirecionamento); AIC_BASE é para espelhos e testes.
PROTO=""; [ -n "${AIC_BASE:-}" ] || PROTO="--proto =https --tlsv1.2"
STAGE=$(mktemp -d); trap 'rm -rf "$STAGE"' EXIT
fetch() { # fetch <arquivo> : baixa para $STAGE e confere o SHA-256
  curl -fL $PROTO --progress-bar -o "$STAGE/$1" "$BASE/$1" || die "$(t "não consegui baixar $1 de $BASE" "could not download $1 from $BASE")"
  want=$(awk -v f="$1" '$2==f {print $1}' "$STAGE/SHA256SUMS")
  [ -n "$want" ] || die "$(t "$1 não consta no SHA256SUMS da release" "$1 is not listed in the release SHA256SUMS")"
  have=$(sha256sum "$STAGE/$1" | cut -d' ' -f1)
  [ "$want" = "$have" ] || die "$(t "$1 não confere com o SHA256SUMS (corrompido ou adulterado)" "$1 does not match SHA256SUMS (corrupted or tampered)")"
}
curl -fsSL $PROTO -o "$STAGE/SHA256SUMS" "$BASE/SHA256SUMS" || die "$(t "não consegui ler a release em $BASE" "could not read the release at $BASE")"
curl -fsSL $PROTO -o "$STAGE/VERSION" "$BASE/VERSION" || die "$(t 'a release não tem VERSION' 'the release has no VERSION')"
REL=$(cat "$STAGE/VERSION")
printf '\n'; banner "$(t "Baixando a versão $REL" "Downloading version $REL")"

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
