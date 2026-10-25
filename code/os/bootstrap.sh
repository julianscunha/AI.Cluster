#!/bin/sh
# Bootstrap de um Linux existente até virar worker do cluster (F15, J3).
#
#   sudo ./bootstrap.sh --controller https://controller:50051 --join-token-file /raiz/token \
#        [--binary ./ai-cluster-worker] [--node-id nome] [--prefix /opt/ai-cluster] \
#        [--model-dir /var/lib/ai-cluster/models] [--insecure] [--force]
#
# O que faz: cria o usuário `ai-cluster`, instala o binário do worker (dono root), grava o
# ambiente em `<prefix>/worker.env` (root:ai-cluster, 0640: o token de join mora ali) e sobe o
# worker, com systemd se houver, senão em segundo plano. Rodar de novo atualiza o binário e o
# ambiente. Trocar o --node-id de uma máquina já instalada exige --force (o nó antigo ficaria
# órfão no controller). O worker entra no cluster pelo caminho de sempre (join + aprovação);
# este script não aprova.
#
# Todos os valores são validados contra uma lista de caracteres permitidos antes de qualquer
# escrita: eles entram em um arquivo de ambiente, numa unit systemd e num comando, e um
# caractere de shell ou uma quebra de linha ali viraria comando.
set -eu

CONTROLLER=""
JOIN_TOKEN=""
TOKEN_FILE=""
BINARY="./ai-cluster-worker"
NODE_ID=""
PREFIX="/opt/ai-cluster"
MODEL_DIR=""
INSECURE=0
FORCE=0

while [ $# -gt 0 ]; do
  case "$1" in
    --controller) CONTROLLER=$2; shift 2 ;;
    --join-token) JOIN_TOKEN=$2; shift 2 ;;
    --join-token-file) TOKEN_FILE=$2; shift 2 ;;
    --binary) BINARY=$2; shift 2 ;;
    --node-id) NODE_ID=$2; shift 2 ;;
    --prefix) PREFIX=$2; shift 2 ;;
    --model-dir) MODEL_DIR=$2; shift 2 ;;
    --insecure) INSECURE=1; shift ;;
    --force) FORCE=1; shift ;;
    *) echo "argumento desconhecido: $1" >&2; exit 2 ;;
  esac
done

die() { echo "$1" >&2; exit 2; }

# O token por arquivo evita pô-lo na linha de comando (visível em `ps` e no histórico).
if [ -n "$TOKEN_FILE" ]; then
  [ -f "$TOKEN_FILE" ] || die "arquivo de token não encontrado: $TOKEN_FILE"
  JOIN_TOKEN=$(head -n 1 "$TOKEN_FILE")
fi

[ -n "$CONTROLLER" ] || die "falta --controller"
[ -n "$JOIN_TOKEN" ] || die "falta --join-token ou --join-token-file"
[ -f "$BINARY" ] || die "binário não encontrado: $BINARY"
[ "$(id -u)" = 0 ] || { echo "rode como root" >&2; exit 1; }
[ -n "$NODE_ID" ] || NODE_ID=$(hostname)

# Lista de permitidos: letras, dígitos e . _ : / @ = + - (sem espaço, aspas, $, `, \, # ou ;).
# `case` com um conjunto negado recusa qualquer caractere de fora, quebra de linha incluída.
allowed() { # allowed <nome> <valor>
  case "$2" in
    *[!A-Za-z0-9._:/@=+-]*) die "$1 tem caractere não permitido (use letras, dígitos e . _ : / @ = + -)" ;;
  esac
}
allowed --controller "$CONTROLLER"
allowed "o token" "$JOIN_TOKEN"
allowed --node-id "$NODE_ID"
allowed --prefix "$PREFIX"
[ -z "$MODEL_DIR" ] || allowed --model-dir "$MODEL_DIR"
case "$PREFIX" in /*) ;; *) die "--prefix deve ser um caminho absoluto" ;; esac
[ -z "$MODEL_DIR" ] || case "$MODEL_DIR" in /*) ;; *) die "--model-dir deve ser um caminho absoluto" ;; esac
case "$CONTROLLER" in
  https://*) ;;
  http://*) [ "$INSECURE" = 1 ] || die "http:// exige --insecure (rede fechada; o token trafega em claro)" ;;
  *) die "--controller deve começar com https:// ou http://" ;;
esac

ENV_FILE="$PREFIX/worker.env"
if [ -f "$ENV_FILE" ] && [ "$FORCE" != 1 ]; then
  OLD_ID=$(sed -n 's/^AI_CLUSTER_NODE_ID=//p' "$ENV_FILE" | head -n 1)
  if [ -n "$OLD_ID" ] && [ "$OLD_ID" != "$NODE_ID" ]; then
    die "esta máquina já é o nó '$OLD_ID'; trocar para '$NODE_ID' deixa o antigo órfão no controller (use --force)"
  fi
fi

# Usuário sem login. O diretório do worker é o `state/`, não o prefixo: o prefixo e o binário
# pertencem ao root, então o worker não consegue trocar o próprio executável.
if ! id ai-cluster >/dev/null 2>&1; then
  useradd --system --no-create-home --home-dir "$PREFIX/state" --shell /usr/sbin/nologin ai-cluster \
    || adduser --system --no-create-home --home "$PREFIX/state" --shell /usr/sbin/nologin ai-cluster
fi

umask 022
# Um prefixo que já existe como link, ou que não é do root, não é aceito: o root escreveria onde
# o dono do link quisesse.
if [ -e "$PREFIX" ]; then
  { [ -d "$PREFIX" ] && [ ! -L "$PREFIX" ]; } || die "$PREFIX existe e não é um diretório comum"
  [ "$(stat -c %u "$PREFIX")" = 0 ] || die "$PREFIX não pertence ao root"
fi
mkdir -p "$PREFIX/bin" "$PREFIX/state"
chown root:root "$PREFIX" "$PREFIX/bin"
chmod 0755 "$PREFIX" "$PREFIX/bin"
# Só o diretório de estado é do worker; sem -R, para não seguir links que ele mesmo plantou.
chown ai-cluster "$PREFIX/state"
chmod 0700 "$PREFIX/state"

install -o root -g root -m 0755 "$BINARY" "$PREFIX/bin/ai-cluster-worker.new"
mv -f "$PREFIX/bin/ai-cluster-worker.new" "$PREFIX/bin/ai-cluster-worker"

# O ambiente é do root e legível pelo grupo do worker: o serviço lê o token, não o reescreve.
{
  echo "AI_CLUSTER_CONTROLLER=$CONTROLLER"
  echo "AI_CLUSTER_JOIN_TOKEN=$JOIN_TOKEN"
  echo "AI_CLUSTER_NODE_ID=$NODE_ID"
  echo "AI_CLUSTER_NODE_CREDENTIAL_FILE=$PREFIX/state/credential"
  [ "$INSECURE" = 1 ] && echo "AI_CLUSTER_INSECURE=1"
  [ -n "$MODEL_DIR" ] && echo "AI_CLUSTER_MODEL_DIR=$MODEL_DIR"
  true
} > "$ENV_FILE.new"
chown root:ai-cluster "$ENV_FILE.new"
chmod 0640 "$ENV_FILE.new"
mv -f "$ENV_FILE.new" "$ENV_FILE"

if [ -d /run/systemd/system ]; then
  cat > /etc/systemd/system/ai-cluster-worker.service <<UNIT
[Unit]
Description=AI Cluster worker
After=network-online.target
Wants=network-online.target

[Service]
User=ai-cluster
EnvironmentFile=$ENV_FILE
ExecStart=$PREFIX/bin/ai-cluster-worker
Restart=always
RestartSec=3
UMask=0077
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
PrivateDevices=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
RestrictSUIDSGID=true
LockPersonality=true
CapabilityBoundingSet=
ReadWritePaths=$PREFIX/state

[Install]
WantedBy=multi-user.target
UNIT
  systemctl daemon-reload
  systemctl enable ai-cluster-worker
  systemctl restart ai-cluster-worker
  echo "worker $NODE_ID instalado e iniciado pelo systemd"
else
  # Sem systemd (contêiner, sistema mínimo): sobe em segundo plano, com o mesmo ambiente. Log e
  # pid ficam em `state/`, a única pasta que o usuário do worker pode escrever.
  PIDFILE="$PREFIX/state/worker.pid"
  # O pid vem de um arquivo que o worker pode escrever: só vale se o processo é mesmo o worker.
  OLD=$(cat "$PIDFILE" 2>/dev/null || true)
  case "$OLD" in
    ''|*[!0-9]*) OLD="" ;;
  esac
  if [ -n "$OLD" ] && grep -q '^ai-cluster-work' "/proc/$OLD/comm" 2>/dev/null; then
    kill "$OLD" 2>/dev/null || true
    # Espera sair de verdade (um zumbi sem init que o recolha já está parado).
    for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
      grep -q '^State:.*[RSD]' "/proc/$OLD/status" 2>/dev/null || break
      sleep 0.5
    done
  fi
  # Os valores seguem como argumentos do `su`, nunca dentro da string do comando.
  set --
  while IFS= read -r line; do
    set -- "$@" "$line"
  done < "$ENV_FILE"
  su -s /bin/sh ai-cluster -c 'state=$1; bin=$2; shift 2; cd "$state"; \
    setsid env "$@" "$bin" >> "$state/worker.log" 2>&1 < /dev/null & \
    echo $! > "$state/worker.pid"' sh "$PREFIX/state" "$PREFIX/bin/ai-cluster-worker" "$@"
  echo "worker $NODE_ID iniciado em segundo plano (sem systemd); log em $PREFIX/state/worker.log"
fi
echo "falta, se o controller exige aprovação: ai-cluster approve $NODE_ID"
