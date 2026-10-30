#!/bin/sh
# Bootstrap de um Linux existente até virar worker do cluster (F15, J3).
#
#   sudo ./bootstrap.sh --controller https://controller:50051 --join-token-file /raiz/token \
#        [--binary ./ai-cluster-worker] [--node-id nome] [--prefix /opt/ai-cluster] \
#        [--model-dir /var/lib/ai-cluster/models] [--insecure] [--force] [--health-timeout 90]
#        [--rpc-server-bin /usr/local/bin/ggml-rpc-server --rpc-tls-dir /etc/ai-cluster/rpc-tls]
#
# `--rpc-server-bin` e `--rpc-tls-dir` ligam o RPC do llama.cpp neste nó (modelos GGUF, ADR-022): o
# worker roda o `ggml-rpc-server` em loopback atrás de um túnel mTLS. A pasta de TLS tem de ter
# `worker.crt`, `worker.key` e `ca.crt` (a CA que assina só o gateway); sem os três o RPC fica desligado.
#
# O que faz: cria o usuário `ai-cluster`, instala o binário do worker (dono root), grava o
# ambiente em `<prefix>/worker.env` (root:ai-cluster, 0640: o token de join mora ali) e sobe o
# worker, com systemd se houver, senão em segundo plano. Rodar de novo atualiza o binário e o
# ambiente. Trocar o --node-id de uma máquina já instalada exige --force (o nó antigo ficaria
# órfão no controller). O worker entra no cluster pelo caminho de sempre (join + aprovação);
# este script não aprova.
#
# Atualização (F19): rodar de novo com outro binário troca o worker e guarda o anterior em
# `bin/ai-cluster-worker.prev`. Com systemd, a versão nova só vale se o serviço ficar ativo e
# o worker se registrar no controller dentro de --health-timeout segundos (0 desliga a espera);
# senão o anterior volta, o serviço reinicia e o script sai com código 3.
#
# Todos os valores são validados contra uma lista de caracteres permitidos antes de qualquer
# escrita: eles entram em um arquivo de ambiente, numa unit systemd e num comando, e um
# caractere de shell ou uma quebra de linha ali viraria comando.
# Idioma das mensagens: AIC_LANG=pt|en (padrão pt). O convite já o fixa ao entrar como worker.
set -eu

LANG_AIC=${AIC_LANG:-pt}
case "$LANG_AIC" in pt|en) ;; *) echo "AIC_LANG deve ser pt ou en / AIC_LANG must be pt or en" >&2; exit 2 ;; esac
# t <pt> <en>: a frase no idioma escolhido.
t() { if [ "$LANG_AIC" = en ]; then printf '%s' "$2"; else printf '%s' "$1"; fi; }

CONTROLLER=""
JOIN_TOKEN=""
TOKEN_FILE=""
BINARY="./ai-cluster-worker"
NODE_ID=""
PREFIX="/opt/ai-cluster"
MODEL_DIR=""
RPC_BIN=""
RPC_TLS_DIR=""
INSECURE=0
FORCE=0
HEALTH_TIMEOUT=90

while [ $# -gt 0 ]; do
  case "$1" in
    --controller) CONTROLLER=$2; shift 2 ;;
    --join-token) JOIN_TOKEN=$2; shift 2 ;;
    --join-token-file) TOKEN_FILE=$2; shift 2 ;;
    --binary) BINARY=$2; shift 2 ;;
    --node-id) NODE_ID=$2; shift 2 ;;
    --prefix) PREFIX=$2; shift 2 ;;
    --model-dir) MODEL_DIR=$2; shift 2 ;;
    --rpc-server-bin) RPC_BIN=$2; shift 2 ;;
    --rpc-tls-dir) RPC_TLS_DIR=$2; shift 2 ;;
    --insecure) INSECURE=1; shift ;;
    --force) FORCE=1; shift ;;
    --health-timeout) HEALTH_TIMEOUT=$2; shift 2 ;;
    *) echo "$(t "argumento desconhecido: $1" "unknown argument: $1")" >&2; exit 2 ;;
  esac
done

die() { echo "$1" >&2; exit 2; }

# O token por arquivo evita pô-lo na linha de comando (visível em `ps` e no histórico).
if [ -n "$TOKEN_FILE" ]; then
  [ -f "$TOKEN_FILE" ] || die "$(t "arquivo de token não encontrado: $TOKEN_FILE" "token file not found: $TOKEN_FILE")"
  JOIN_TOKEN=$(head -n 1 "$TOKEN_FILE")
fi

[ -n "$CONTROLLER" ] || die "$(t 'falta --controller' 'missing --controller')"
[ -n "$JOIN_TOKEN" ] || die "$(t 'falta --join-token ou --join-token-file' 'missing --join-token or --join-token-file')"
[ -f "$BINARY" ] || die "$(t "binário não encontrado: $BINARY" "binary not found: $BINARY")"
[ "$(id -u)" = 0 ] || { echo "$(t 'rode como root' 'run as root')" >&2; exit 1; }
[ -n "$NODE_ID" ] || NODE_ID=$(hostname)
case "$HEALTH_TIMEOUT" in ''|*[!0-9]*) die "$(t '--health-timeout deve ser um número de segundos' '--health-timeout must be a number of seconds')" ;; esac

# Lista de permitidos: letras, dígitos e . _ : / @ = + - (sem espaço, aspas, $, `, \, # ou ;).
# `case` com um conjunto negado recusa qualquer caractere de fora, quebra de linha incluída.
allowed() { # allowed <nome> <valor>
  case "$2" in
    *[!A-Za-z0-9._:/@=+-]*) die "$(t "$1 tem caractere não permitido (use letras, dígitos e . _ : / @ = + -)" "$1 has a character not allowed (use letters, digits and . _ : / @ = + -)")" ;;
  esac
}
allowed --controller "$CONTROLLER"
allowed "$(t 'o token' 'the token')" "$JOIN_TOKEN"
allowed --node-id "$NODE_ID"
allowed --prefix "$PREFIX"
[ -z "$MODEL_DIR" ] || allowed --model-dir "$MODEL_DIR"
[ -z "$RPC_BIN" ] || allowed --rpc-server-bin "$RPC_BIN"
[ -z "$RPC_TLS_DIR" ] || allowed --rpc-tls-dir "$RPC_TLS_DIR"
# RPC é tudo ou nada: o binário e a pasta de TLS vêm juntos, com os três arquivos.
if [ -n "$RPC_BIN" ] || [ -n "$RPC_TLS_DIR" ]; then
  [ -n "$RPC_BIN" ] && [ -n "$RPC_TLS_DIR" ] || die "$(t '--rpc-server-bin e --rpc-tls-dir vêm juntos' '--rpc-server-bin and --rpc-tls-dir come together')"
  case "$RPC_BIN" in /*) ;; *) die "$(t '--rpc-server-bin deve ser um caminho absoluto' '--rpc-server-bin must be an absolute path')" ;; esac
  case "$RPC_TLS_DIR" in /*) ;; *) die "$(t '--rpc-tls-dir deve ser um caminho absoluto' '--rpc-tls-dir must be an absolute path')" ;; esac
  [ -x "$RPC_BIN" ] || die "$(t "--rpc-server-bin não é um executável: $RPC_BIN" "--rpc-server-bin is not an executable: $RPC_BIN")"
  for f in worker.crt worker.key ca.crt; do
    [ -r "$RPC_TLS_DIR/$f" ] || die "$(t "falta $RPC_TLS_DIR/$f (o RPC não vai em texto puro)" "missing $RPC_TLS_DIR/$f (the RPC does not go in plain text)")"
  done
fi
case "$PREFIX" in /*) ;; *) die "$(t '--prefix deve ser um caminho absoluto' '--prefix must be an absolute path')" ;; esac
[ -z "$MODEL_DIR" ] || case "$MODEL_DIR" in /*) ;; *) die "$(t '--model-dir deve ser um caminho absoluto' '--model-dir must be an absolute path')" ;; esac
case "$CONTROLLER" in
  https://*) ;;
  http://*) [ "$INSECURE" = 1 ] || die "$(t 'http:// exige --insecure (rede fechada; o token trafega em claro)' 'http:// requires --insecure (closed network; the token travels in clear text)')" ;;
  *) die "$(t '--controller deve começar com https:// ou http://' '--controller must start with https:// or http://')" ;;
esac

ENV_FILE="$PREFIX/worker.env"
if [ -f "$ENV_FILE" ] && [ "$FORCE" != 1 ]; then
  OLD_ID=$(sed -n 's/^AI_CLUSTER_NODE_ID=//p' "$ENV_FILE" | head -n 1)
  if [ -n "$OLD_ID" ] && [ "$OLD_ID" != "$NODE_ID" ]; then
    die "$(t "esta máquina já é o nó '$OLD_ID'; trocar para '$NODE_ID' deixa o antigo órfão no controller (use --force)" "this machine is already node '$OLD_ID'; switching to '$NODE_ID' leaves the old one orphaned on the controller (use --force)")"
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
  { [ -d "$PREFIX" ] && [ ! -L "$PREFIX" ]; } || die "$(t "$PREFIX existe e não é um diretório comum" "$PREFIX exists and is not a regular directory")"
  [ "$(stat -c %u "$PREFIX")" = 0 ] || die "$(t "$PREFIX não pertence ao root" "$PREFIX does not belong to root")"
fi
mkdir -p "$PREFIX/bin" "$PREFIX/state"
chown root:root "$PREFIX" "$PREFIX/bin"
chmod 0755 "$PREFIX" "$PREFIX/bin"
# Só o diretório de estado é do worker; sem -R, para não seguir links que ele mesmo plantou.
chown ai-cluster "$PREFIX/state"
chmod 0700 "$PREFIX/state"

install -o root -g root -m 0755 "$BINARY" "$PREFIX/bin/ai-cluster-worker.new"
# A versão em uso vira a de volta: só ela, a anterior a essa se perde.
HAS_PREV=0
if [ -f "$PREFIX/bin/ai-cluster-worker" ]; then
  install -o root -g root -m 0755 "$PREFIX/bin/ai-cluster-worker" "$PREFIX/bin/ai-cluster-worker.prev"
  HAS_PREV=1
fi
mv -f "$PREFIX/bin/ai-cluster-worker.new" "$PREFIX/bin/ai-cluster-worker"

# O ambiente é do root e legível pelo grupo do worker: o serviço lê o token, não o reescreve.
{
  echo "AI_CLUSTER_CONTROLLER=$CONTROLLER"
  echo "AI_CLUSTER_JOIN_TOKEN=$JOIN_TOKEN"
  echo "AI_CLUSTER_NODE_ID=$NODE_ID"
  echo "AI_CLUSTER_NODE_CREDENTIAL_FILE=$PREFIX/state/credential"
  [ "$INSECURE" = 1 ] && echo "AI_CLUSTER_INSECURE=1"
  [ -n "$MODEL_DIR" ] && echo "AI_CLUSTER_MODEL_DIR=$MODEL_DIR"
  if [ -n "$RPC_BIN" ]; then
    echo "AI_CLUSTER_RPC_SERVER_BIN=$RPC_BIN"
    echo "AI_CLUSTER_RPC_TLS_CERT=$RPC_TLS_DIR/worker.crt"
    echo "AI_CLUSTER_RPC_TLS_KEY=$RPC_TLS_DIR/worker.key"
    echo "AI_CLUSTER_RPC_TLS_CLIENT_CA=$RPC_TLS_DIR/ca.crt"
  fi
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
  SINCE=$(date '+%Y-%m-%d %H:%M:%S')
  systemctl restart ai-cluster-worker
  echo "$(t "worker $NODE_ID instalado e iniciado pelo systemd" "worker $NODE_ID installed and started by systemd")"
  # Saudável é: ativo e registrado no controller depois deste restart. Um binário que cai em
  # laço de reinício nunca chega ao registro.
  healthy() {
    systemctl is-active --quiet ai-cluster-worker       && journalctl -u ai-cluster-worker --since "$SINCE" --no-pager 2>/dev/null         | grep -q "registrado node_id=.* aceito=true"
  }
  if [ "$HEALTH_TIMEOUT" -gt 0 ]; then
    waited=0
    until healthy; do
      if [ "$waited" -ge "$HEALTH_TIMEOUT" ]; then
        if [ "$HAS_PREV" = 1 ]; then
          echo "$(t "a versão nova não ficou saudável em ${HEALTH_TIMEOUT}s; voltando à anterior" "the new version was not healthy in ${HEALTH_TIMEOUT}s; rolling back to the previous one")" >&2
          install -o root -g root -m 0755 "$PREFIX/bin/ai-cluster-worker.prev" "$PREFIX/bin/ai-cluster-worker.new"
          mv -f "$PREFIX/bin/ai-cluster-worker.new" "$PREFIX/bin/ai-cluster-worker"
          systemctl restart ai-cluster-worker
          echo "$(t 'rollback feito' 'rollback done')" >&2
        else
          echo "$(t "o worker não ficou saudável em ${HEALTH_TIMEOUT}s e não há versão anterior para voltar" "the worker was not healthy in ${HEALTH_TIMEOUT}s and there is no previous version to roll back to")" >&2
        fi
        exit 3
      fi
      sleep 2
      waited=$((waited + 2))
    done
    echo "$(t 'worker saudável (registrado no controller)' 'worker healthy (registered with the controller)')"
  fi
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
  echo "$(t "worker $NODE_ID iniciado em segundo plano (sem systemd); log em $PREFIX/state/worker.log" "worker $NODE_ID started in the background (no systemd); log in $PREFIX/state/worker.log")"
fi
[ "${AIC_INVITED:-0}" = 1 ] || echo "$(t "falta, se o controller exige aprovação: ai-cluster approve $NODE_ID --fingerprint <impressão que o worker imprime>" "missing, if the controller requires approval: ai-cluster approve $NODE_ID --fingerprint <fingerprint the worker prints>")"
