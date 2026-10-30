#!/usr/bin/env bash
# Instalador guiado (install.sh + setup + o comando `aic`): servidor, worker e gateway noutro computador,
# em contêineres Debian 12 (sem systemd, e com um systemd de mentira onde o teste diz), com um espelho local
# no lugar da release do GitHub.
#
#   bash code/scripts/package-release.sh ../../temp/rel10 && bash code/tests/phase-u-setup.sh ../../temp/rel10
#
# Critérios verificados:
#   A) install.sh instala o servidor (controller + gateway) baixando só os pacotes desse papel, com o
#      cabeçalho do instalador e o aviso de que, sem systemd, nada volta sozinho depois de um reinício;
#   B) o convite entrega o pacote do worker pelo processador e o nó fica READY e aprovado;
#   B1) um worker com systemd de mentira: `enable` é chamado e a unit tem Restart=always;
#   B2) o /wait do convite apresenta um computador com Linux para instalação e o operador o vê;
#   B2b) `aic worker change` muda um nó de controller pelo convite (token, CA, certificado novo; falha não altera nada);
#   B3) o worker exige o código do convite (8 caracteres) e o convite trava com chutes demais;
#   C) o controller sozinho e o gateway em outro computador sobem (o gateway com o controller dele);
#   C2) aic update apply no controller: atualiza os binários e mantém token, certificados e config;
#   D) processador errado, gateway em ARM de 32 bits e ARM de 64 bits são recusados com mensagem;
#      --lang fr é recusado e --mode dedicated (alias em inglês) funciona;
#   E) em inglês (--lang en): banner e recusa do install.sh, idioma salvo lido pelo setup e o
#      convite gera join e wait em inglês;
#   G) controller e gateway com systemd de mentira: habilitados no boot, Restart=always e WantedBy;
#   H) aic update no worker Linux: status pela origem, apply troca os binários, guarda os anteriores e o worker volta;
#   I) cabeçalho com o nome do componente, cor num terminal, comandos antigos inexistentes e
#      `aic worker role` recusado no Linux comum.
set -uo pipefail
export MSYS_NO_PATHCONV=1
DIST=$(cd "${1:-dist/release}" && pwd -W 2>/dev/null || pwd)
NET=aic-st
BASE=http://aic-mirror:9000
teardown() { docker rm -f aic-mirror aic-srv aic-wk aic-ctl aic-gw aic-sdc aic-sdw >/dev/null 2>&1; docker network rm $NET >/dev/null 2>&1; }
fail() { echo "FALHA: $1"; docker exec aic-srv tail -5 /tmp/invite.log 2>&1; teardown; exit 1; }
trap teardown EXIT
teardown; docker network create $NET >/dev/null
# Imagem base com as dependências do instalador (construída uma vez; o cache do Docker a reaproveita).
docker build -q -t aic-test-base - >/dev/null <<'DOCKERFILE' || { echo "FALHA: imagem base"; exit 1; }
FROM debian:12
RUN apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq openssl libgomp1 iproute2 python3 curl wget procps >/dev/null && rm -rf /var/lib/apt/lists/*
DOCKERFILE
docker run -d --name aic-mirror --network $NET -v "$DIST:/r:ro" -w /r aic-test-base python3 -m http.server 9000 >/dev/null
for n in aic-srv aic-wk aic-ctl aic-gw aic-sdc aic-sdw; do docker run -d --name $n --network $NET aic-test-base sleep 900 >/dev/null; done
for n in aic-srv aic-wk aic-ctl aic-gw aic-sdc aic-sdw; do
  for _ in $(seq 60); do docker exec $n curl -fsS $BASE/install.sh -o /tmp/install.sh 2>/dev/null && break; sleep 2; done
  docker exec $n test -s /tmp/install.sh || fail "sem install.sh no espelho"
done
inst() { n=$1; shift; docker exec $n sh -c "AIC_BASE=$BASE sh /tmp/install.sh --mode linux --kind conteiner --yes $*" 2>&1; }
# systemctl e journalctl de mentira: registram as chamadas, e o start/restart sobe o ExecStart da unit com o
# EnvironmentFile dela (o processo é o de verdade). O journalctl mostra o registro que o serviço gravou.
fake_systemd() {
  docker exec -i "$1" sh -c 'mkdir -p /run/systemd/system /etc/systemd/system && cat > /usr/local/bin/systemctl && chmod 755 /usr/local/bin/systemctl' <<'FAKE'
#!/bin/sh
echo "$*" >> /var/log/fake-systemctl.log
mkdir -p /run/fake-units /var/log/fake-units
for svc in "$@"; do :; done
case "$1" in
is-active) pid=$(cat /run/fake-units/$svc.pid 2>/dev/null); [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null ;;
start|restart)
  f=/etc/systemd/system/$svc.service
  pid=$(cat /run/fake-units/$svc.pid 2>/dev/null); [ -n "$pid" ] && kill "$pid" 2>/dev/null; sleep 1
  envf=$(sed -n 's/^EnvironmentFile=//p' "$f"); exe=$(sed -n 's/^ExecStart=//p' "$f")
  ( set -a; . "$envf"; set +a; nohup "$exe" >> /var/log/fake-units/$svc.log 2>&1 </dev/null & echo $! > /run/fake-units/$svc.pid ) ;;
esac
exit 0
FAKE
  docker exec -i "$1" sh -c 'cat > /usr/local/bin/journalctl && chmod 755 /usr/local/bin/journalctl' <<'FAKE'
#!/bin/sh
svc=$(echo "$*" | sed -n 's/.*-u \([^ ]*\).*/\1/p')
cat /var/log/fake-units/$svc.log 2>/dev/null
exit 0
FAKE
}

echo "=== A) servidor completo baixa só controller, gateway e tools ==="
OUT=$(inst aic-srv --role server --arch x86_64) || { echo "$OUT"; fail "install.sh server"; }
echo "$OUT" | grep -q "✔ gateway-linux-x86_64" || fail "o gateway deveria ter sido baixado"
echo "$OUT" | grep -q "worker-linux" && fail "o servidor não deveria baixar o worker"
docker exec aic-srv sh -c 'grep -q "^AI_CLUSTER_GATEWAY_URL=http://$(cat /etc/ai-cluster/server-ip):8080$" /etc/ai-cluster/gateway.env' || fail "o gateway do servidor deveria anunciar http://<ip>:8080 (e não 0.0.0.0)"
echo "$OUT" | grep -q "^Real.AI.Cluster instalador - " || fail "o instalador deveria abrir com o cabeçalho dele"
echo "$OUT" | grep -q "não voltam sozinhos" || fail "sem systemd o instalador deveria avisar que nada volta sozinho"
docker exec aic-srv sh -c 'test -x /opt/ai-cluster/libexec/setup && test -x /usr/local/bin/aic && test -x /opt/ai-cluster/bin/aic' || fail "o setup e o aic deveriam ficar em /opt/ai-cluster"
echo "OK: servidor instalado"

echo "=== B) o convite entrega o worker e o nó fica READY e aprovado ==="
docker exec -d aic-srv sh -c "aic cluster invite --minutes 2 > /tmp/invite.log 2>&1"; sleep 25
SIP=$(docker exec aic-srv cat /etc/ai-cluster/server-ip)
WCODE=$(docker exec aic-srv sh -c "grep -o -- '--code [A-Za-z0-9]*' /tmp/invite.log | head -n 1 | cut -d' ' -f2")
[ ${#WCODE} -eq 8 ] || fail "o convite deveria mostrar um código de 8 caracteres (veio: $WCODE)"
docker exec aic-srv grep -q "^Real.AI.Cluster convite " /tmp/invite.log || fail "o convite deveria abrir com o cabeçalho dele"
# O worker precisa do código: sem ele o instalador recusa, o /join antigo não existe e o /sign nega.
inst aic-wk --role worker --arch x86_64 --server "$SIP" >/dev/null 2>&1 && fail "o worker sem código deveria ser recusado"
[ "$(docker exec aic-wk curl -s -o /dev/null -w '%{http_code}' http://$SIP:8099/join)" = 404 ] || fail "o /join sem código não deveria existir"
[ "$(docker exec aic-wk curl -s -o /dev/null -w '%{http_code}' -X POST --data x "http://$SIP:8099/sign?name=intruso&code=ZZZZZZZZ")" = 403 ] || fail "o /sign com código errado deveria dar 403"
docker exec aic-wk sh -c "AIC_CODE=$(echo $WCODE | tr A-Z a-z) AIC_BASE=$BASE sh /tmp/install.sh --mode linux --kind conteiner --yes --role worker --arch x86_64 --server $SIP" >/dev/null 2>&1 || fail "o worker não entrou"
for _ in $(seq 20); do
  docker exec aic-srv aic cluster nodes 2>/dev/null | grep -E "READY.*aprovado" >/dev/null && break; sleep 2
done
docker exec aic-srv aic cluster nodes 2>/dev/null | grep -E "READY.*aprovado" >/dev/null || fail "o nó não ficou READY e aprovado"
echo "OK: worker READY e aprovado"
# Certificado por nó: a chave nasceu no worker (o servidor não tem chave de worker) e o SAN leva o nome do nó.
docker exec aic-srv test ! -e /etc/ai-cluster/pki/worker.key || fail "o servidor não deveria guardar chave de worker"
WNAME=$(docker exec aic-wk hostname | sed 's/[^A-Za-z0-9.-]/-/g' | cut -c1-40)
docker exec aic-wk sh -c "openssl x509 -in /etc/ai-cluster/rpc-tls/worker.crt -noout -ext subjectAltName | grep -q 'DNS:aic-worker-$WNAME' && openssl verify -CAfile /etc/ai-cluster/rpc-tls/ca.crt /etc/ai-cluster/rpc-tls/worker.crt >/dev/null"   || fail "o certificado do worker deveria ter o SAN aic-worker-$WNAME e conferir com a CA"
docker exec aic-srv grep -q 'AI_CLUSTER_LLAMA_RPC_SERVER_NAME=aic-worker-{node_id}' /etc/ai-cluster/gateway.env || fail "o gateway deveria esperar o nome por nó"
echo "OK: certificado por nó"
# O worker entrou pelo join novo: ele grava a release (origem da atualização) e leva o aic e o setup.
docker exec aic-wk sh -c 'grep -q "^ROLE=worker$" /etc/ai-cluster/release && grep -q "^BASE=http" /etc/ai-cluster/release && grep -q "^SUMS=[0-9a-f]" /etc/ai-cluster/release && test -x /opt/ai-cluster/libexec/setup && test -x /opt/ai-cluster/bin/aic' || fail "o join deveria gravar /etc/ai-cluster/release e instalar o aic e o setup"
echo "OK: join grava a release e instala aic e setup"

echo "=== B1) worker com systemd de mentira: habilitado no boot, com Restart=always ==="
fake_systemd aic-sdw
inst aic-sdw --role worker --arch x86_64 --server "$SIP" >/dev/null 2>&1 && fail "o worker sem código deveria ser recusado (systemd)"
docker exec aic-sdw sh -c "AIC_CODE=$(echo $WCODE | tr A-Z a-z) AIC_BASE=$BASE sh /tmp/install.sh --mode linux --kind conteiner --yes --role worker --arch x86_64 --server $SIP" >/dev/null 2>&1 || fail "o worker com systemd não entrou"
docker exec aic-sdw grep -qx 'enable ai-cluster-worker' /var/log/fake-systemctl.log || fail "o worker deveria ser habilitado no boot (systemctl enable)"
docker exec aic-sdw grep -qx '^Restart=always$' /etc/systemd/system/ai-cluster-worker.service || fail "a unit do worker deveria ter Restart=always"
docker exec aic-sdw grep -qx '^WantedBy=multi-user.target$' /etc/systemd/system/ai-cluster-worker.service || fail "a unit do worker deveria ser WantedBy=multi-user.target"
docker exec aic-sdw sh -c 'kill -0 $(cat /run/fake-units/ai-cluster-worker.pid)' || fail "o worker com systemd deveria estar rodando"
echo "OK B1: worker habilitado no boot e unit com Restart=always"

echo "=== B2) quem já tem Linux se apresenta pelo /wait e aparece para aprovar ==="
# O convite que está aberto também abriu a porta de espera; o script baixa só o aic-wait e o roda.
docker exec aic-wk sh -c "curl -fsSL http://$SIP:8099/wait -o /tmp/wait.sh" || fail "o servidor não serviu o /wait"
docker exec -d aic-wk sh -c "timeout 40 sh /tmp/wait.sh > /tmp/wait.log 2>&1"
for _ in $(seq 20); do
  docker exec aic-srv aic cluster install list 2>/dev/null | grep -q "fingerprint=.* aguardando" && break; sleep 2
done
docker exec aic-srv aic cluster install list | grep -q "fingerprint=.* aguardando" || fail "o aparelho não apareceu em install list: $(docker exec aic-wk tail -5 /tmp/wait.log)"
docker exec aic-wk grep -q "Impressão digital" /tmp/wait.log || fail "a tela de espera não mostrou a impressão digital"
for _ in $(seq 10); do docker exec aic-srv grep -q "quer instalar" /tmp/invite.log && break; sleep 2; done
docker exec aic-srv grep -q "quer instalar" /tmp/invite.log || fail "o convite deveria avisar o operador do aparelho"
echo "OK: aparelho aguardando aprovação"

# `aic worker change` (AI Cluster OS): muda um nó de controller pelo convite, sem regravar a imagem. O nó de
# mentira usa pastas e um rc-service de mentira; o resto (wget, openssl, convite) é de verdade.
AICDIR=$(cd "$(dirname "$0")/../os/image/overlay/usr/libexec/aic" && pwd -W 2>/dev/null || pwd)
docker cp "$AICDIR/change" aic-wk:/tmp/aic-change >/dev/null
docker exec aic-wk sh -c "mkdir -p /tmp/fd/state /tmp/fd/rpc-tls; printf 'AI_CLUSTER_CONTROLLER=http://velho:50051
AI_CLUSTER_JOIN_TOKEN=velho
AI_CLUSTER_NODE_ID=nodechange
AI_CLUSTER_INSECURE=1
' > /tmp/fd/worker.env; echo cred > /tmp/fd/state/credential; printf '#!/bin/sh
echo \$@ >> /tmp/fd/rc.log
' > /tmp/fd/rc; chmod +x /tmp/fd/rc"
CH="AIC_DATA=/tmp/fd AIC_RC=/tmp/fd/rc sh /tmp/aic-change"
docker exec aic-wk sh -c "$CH --server $SIP:8099 --code ZZZZZZZZ" >/dev/null 2>&1 && fail "aic change com código errado deveria falhar"
docker exec aic-wk grep -q 'JOIN_TOKEN=velho' /tmp/fd/worker.env || fail "aic change com falha não pode mexer no nó"
docker exec aic-wk sh -c "$CH --server $SIP:8099 --code $(echo $WCODE | tr A-Z a-z) --controller http://192.168.1.25:50051" >/dev/null || fail "aic change pelo convite"
NEWTOK=$(docker exec aic-srv sed -n 's/^AI_CLUSTER_JOIN_TOKEN=//p' /etc/ai-cluster/controller.env)
docker exec aic-wk grep -q "AI_CLUSTER_JOIN_TOKEN=$NEWTOK" /tmp/fd/worker.env || fail "o token novo deveria estar no worker.env"
docker exec aic-wk grep -q 'AI_CLUSTER_CONTROLLER=http://192.168.1.25:50051' /tmp/fd/worker.env || fail "o --controller deveria valer"
docker exec aic-wk grep -q 'AI_CLUSTER_INSECURE=1' /tmp/fd/worker.env || fail "o resto do worker.env deveria ficar"
docker exec aic-wk sh -c "test ! -e /tmp/fd/state/credential && openssl verify -CAfile /tmp/fd/rpc-tls/ca.crt /tmp/fd/rpc-tls/worker.crt && openssl x509 -in /tmp/fd/rpc-tls/worker.crt -noout -ext subjectAltName | grep -q 'aic-worker-nodechange' && grep -q stop /tmp/fd/rc.log && grep -q start /tmp/fd/rc.log" >/dev/null || fail "aic change deveria trocar credencial e certificado e reiniciar o worker"
echo "OK: aic worker change troca de controller pelo convite"

# Chutes demais travam o convite: o código é curto.
for _ in $(seq 31); do docker exec aic-wk curl -s -o /dev/null "http://$SIP:8099/join-AAAAAAAA"; done
[ "$(docker exec aic-wk curl -s -o /dev/null -w '%{http_code}' http://$SIP:8099/join-$WCODE)" = 429 ] || fail "o convite deveria travar depois de chutes demais"
echo "OK: o worker exige o código e o convite trava com chutes demais"

echo "=== C) controller sozinho e gateway noutro computador ==="
inst aic-ctl --role controller --arch x86_64 >/dev/null || fail "install.sh controller"
docker exec -d aic-ctl sh -c "aic cluster invite --minutes 2 > /tmp/invite.log 2>&1"; sleep 25
CIP=$(docker exec aic-ctl cat /etc/ai-cluster/server-ip)
GCODE=$(docker exec aic-ctl sh -c "grep -o -- '--code [A-Za-z0-9]*' /tmp/invite.log | head -n 1 | cut -d' ' -f2")
[ -n "$GCODE" ] || fail "o convite deveria mostrar o código do gateway"
inst aic-gw --role gateway --arch x86_64 --server "$CIP" >/dev/null 2>&1 && fail "o gateway sem código deveria ser recusado"
docker exec aic-gw sh -c "AIC_CODE=$GCODE AIC_BASE=$BASE sh /tmp/install.sh --mode linux --kind conteiner --yes --role gateway --arch x86_64 --server $CIP" >/dev/null 2>&1 || fail "install.sh gateway"
docker exec aic-gw sh -c "ss -ltn | grep -q ':8080 '" || fail "o gateway não abriu a porta 8080"
docker exec aic-gw grep -q "controller http://$CIP:50051" /var/log/ai-cluster/gateway.log || fail "o gateway deveria apontar para o controller"
docker exec aic-gw sh -c 'grep -q "^AI_CLUSTER_GATEWAY_URL=http://$(hostname -i | cut -d" " -f1):8080$" /etc/ai-cluster/gateway.env' || fail "o gateway noutro computador deveria anunciar o IP dele"
echo "OK: controller e gateway em computadores separados"

echo "=== C2) aic update apply: atualiza os binários e mantém tokens, certificados e config ==="
TOK=$(docker exec aic-ctl sed -n 's/^AI_CLUSTER_JOIN_TOKEN=//p' /etc/ai-cluster/controller.env)
OUT=$(docker exec -e AIC_LANG=pt aic-ctl aic update apply --yes 2>&1)
echo "$OUT" | grep -q "já está na versão mais recente" || { echo "$OUT" | tail -5; fail "sem novidade, o update deveria dizer que já está na versão mais recente"; }
# Simula uma instalação antiga (sem o resumo da release) e uma config de antes das portas novas.
docker exec aic-ctl sh -c "sed -i '/^SUMS=/d' /etc/ai-cluster/release; sed -i '/^AI_CLUSTER_ENROLL_ADDR=/d' /etc/ai-cluster/controller.env"
OUT=$(docker exec aic-ctl aic update apply --yes 2>&1) || { echo "$OUT" | tail -8; fail "aic update apply"; }
echo "$OUT" | grep -q "Atualizado para" || { echo "$OUT" | tail -8; fail "o update deveria terminar com 'Atualizado para'"; }
docker exec aic-ctl aic cluster nodes >/dev/null 2>&1 || fail "o controller deveria responder depois do update"
[ "$(docker exec aic-ctl sed -n 's/^AI_CLUSTER_JOIN_TOKEN=//p' /etc/ai-cluster/controller.env)" = "$TOK" ] || fail "o update não pode trocar o token de join"
docker exec aic-ctl grep -q '^AI_CLUSTER_ENROLL_ADDR=' /etc/ai-cluster/controller.env || fail "o update deveria acrescentar a config nova que faltava"
docker exec aic-ctl test -x /opt/ai-cluster/bin/.prev/ai-cluster-controller || fail "o update deveria guardar os binários anteriores"
docker exec aic-ctl grep -q '^SUMS=' /etc/ai-cluster/release || fail "o update deveria gravar o resumo da release"
echo "OK: aic update apply"

echo "=== D) combinações recusadas ==="
# Capturar a saída antes do grep: `grep -q` fecha o pipe cedo e o pipefail contaria como falha.
refused() { out=$(inst aic-ctl "$@"); case "$out" in *"$MSG"*) ;; *) fail "esperava \"$MSG\" para: $*" ;; esac; }
MSG="só existe para x86_64" refused --role server --arch armv7
MSG="ARM de 64 bits ainda não" refused --role worker --arch aarch64 --server 1.2.3.4
MSG="este computador é x86_64" refused --role worker --arch armv7 --server 1.2.3.4
MSG="idioma inválido" refused --lang fr
echo "OK: recusas com mensagem"
out=$(inst aic-ctl --mode dedicated --lang en)
echo "$out" | grep -qF "Dedicated mode writes" || fail "--mode dedicated (alias) deveria mostrar o modo dedicado em inglês"
echo "OK: --mode dedicated (alias de dedicado)"

echo "=== E) em inglês: install.sh, setup e convite ==="
# O controller (C) já está instalado: o install.sh roda inteiro em inglês e o setup recusa em inglês.
OUT=$(inst aic-ctl --role controller --arch x86_64 --kind container --lang en)
for p in "Welcome" "Summary" "Downloading version" "Container:" "already installed"; do
  echo "$OUT" | grep -qF "$p" || fail "saída em inglês sem \"$p\""
done
for p in "Bem-vindo" "Resumo" "Baixando a versão" "Contêiner:" "já instalado"; do
  echo "$OUT" | grep -qF "$p" && fail "saída em inglês traz \"$p\""
done
echo "OK E1) install.sh em inglês: banner, resumo e recusa traduzidos"
# O idioma fica gravado na instalação e o setup o lê sem AIC_LANG.
[ "$(docker exec aic-ctl cat /etc/ai-cluster/lang)" = pt ] || fail "a instalação deveria gravar /etc/ai-cluster/lang"
docker exec aic-ctl sh -c "echo en > /etc/ai-cluster/lang"
ST=$(docker exec aic-ctl aic controller status 2>&1)
echo "$ST" | grep -qF "Status of this computer" || { echo "$ST" | head -5; fail "aic controller status deveria seguir o idioma salvo (en)"; }
docker exec aic-ctl sh -c "echo pt > /etc/ai-cluster/lang"
echo "OK E2) setup lê o idioma salvo em /etc/ai-cluster/lang"
# Um convite com AIC_LANG=en gera join e wait em inglês (porta 8098, para não colidir com o convite da C).
docker exec -d aic-ctl sh -c "AIC_LANG=en aic cluster invite --minutes 1 --port 8098 > /tmp/invite-en.log 2>&1"
for _ in $(seq 10); do docker exec aic-ctl grep -q -- "--code" /tmp/invite-en.log 2>/dev/null && break; sleep 1; done
ECODE=$(docker exec aic-ctl sh -c "grep -o -- '--code [A-Za-z0-9]*' /tmp/invite-en.log | head -n 1 | cut -d' ' -f2")
[ -n "$ECODE" ] || fail "o convite em inglês deveria mostrar o código"
docker exec aic-ctl curl -fsS "http://$CIP:8098/join-$ECODE" -o /tmp/join-en.sh || fail "o /join-<código> deveria existir"
docker exec aic-ctl sh -c "curl -fsS http://$CIP:8098/wait -o /tmp/wait-en.sh && sh -n /tmp/join-en.sh && sh -n /tmp/wait-en.sh && grep -q 'joins this cluster' /tmp/join-en.sh && grep -q 'dedicated mode' /tmp/wait-en.sh" \
  || fail "o convite em inglês deveria gerar join e wait em inglês"
echo "OK E3) convite em inglês gera join e wait em inglês"

echo "=== G) controller e gateway com systemd de mentira: habilitados no boot ==="
fake_systemd aic-sdc
OUT=$(inst aic-sdc --role server --arch x86_64) || { echo "$OUT" | tail -8; fail "install.sh server com systemd"; }
docker exec aic-sdc grep -qx 'enable ai-cluster-controller' /var/log/fake-systemctl.log || fail "o controller deveria ser habilitado no boot"
docker exec aic-sdc grep -qx 'enable ai-cluster-gateway' /var/log/fake-systemctl.log || fail "o gateway deveria ser habilitado no boot"
docker exec aic-sdc sh -c 'for s in controller gateway; do f=/etc/systemd/system/ai-cluster-$s.service; grep -qx "Restart=always" $f && grep -qx "WantedBy=multi-user.target" $f || exit 1; done' \
  || fail "as units do controller e do gateway deveriam ter Restart=always e WantedBy=multi-user.target"
ST=$(docker exec aic-sdc aic controller status 2>&1)
echo "$ST" | grep -q "ai-cluster-controller: ativo" || { echo "$ST"; fail "aic controller status deveria mostrar o serviço ativo"; }
echo "OK G) controller e gateway habilitados no boot, com Restart=always"

echo "=== H) aic update no worker Linux: status pela origem, apply troca e o worker volta ==="
docker exec aic-wk aic update status | grep -q "já está na versão mais recente" || fail "aic update status do worker deveria comparar com a origem"
OUT=$(docker exec aic-wk aic update apply --force --yes 2>&1) || { echo "$OUT" | tail -8; fail "aic update apply do worker"; }
echo "$OUT" | grep -q "Atualizado para" || { echo "$OUT" | tail -8; fail "o update do worker deveria terminar com 'Atualizado para'"; }
docker exec aic-wk test -x /opt/ai-cluster/bin/.prev/ai-cluster-worker || fail "o update do worker deveria guardar o binário anterior"
docker exec aic-wk test -x /opt/ai-cluster/bin/.prev/aic || fail "o update do worker deveria guardar o aic anterior"
docker exec aic-wk sh -c 'kill -0 $(cat /opt/ai-cluster/state/worker.pid)' || fail "o worker deveria estar rodando depois do update"
for _ in $(seq 20); do
  docker exec aic-wk sh -c 'tail -n 30 /opt/ai-cluster/state/worker.log | grep -q "registrado node_id=.* aceito=true"' && break; sleep 2
done
docker exec aic-wk sh -c 'tail -n 30 /opt/ai-cluster/state/worker.log | grep -q "registrado node_id=.* aceito=true"' || fail "o worker deveria se registrar de novo depois do update"
echo "OK H) update do worker: binários trocados, anteriores guardados, worker de volta"

echo "=== I) cabeçalho com o nome do componente, comandos antigos inexistentes ==="
S=$(docker exec aic-srv aic controller status 2>&1); echo "$S" | head -n 1 | grep -q "^Real.AI.Cluster controller v" || fail "o controller deveria abrir com o cabeçalho dele"
S=$(docker exec aic-gw aic gateway status 2>&1); echo "$S" | head -n 1 | grep -q "^Real.AI.Cluster gateway v" || fail "o gateway deveria abrir com o cabeçalho dele"
S=$(docker exec aic-wk aic worker status 2>&1); echo "$S" | head -n 1 | grep -q "^Real.AI.Cluster worker v" || fail "o worker deveria abrir com o cabeçalho dele"
S=$(docker exec aic-wk aic update status 2>&1); echo "$S" | head -n 1 | grep -q "^Real.AI.Cluster atualização" || fail "o update deveria abrir com o cabeçalho dele"
COR=$(docker exec aic-srv script -qc "aic controller status" /dev/null 2>&1)
echo "$COR" | grep -q "$(printf '\033')\[1;97mReal.AI.Cluster controller" \
  || fail "com terminal, o cabeçalho deveria sair colorido (título em negrito)"
echo "OK I1) cabeçalho com o nome do componente, e colorido num terminal"
for c in ai-cluster aic-setup aic-change aic-role aic-passwd; do
  [ -z "$(docker exec aic-srv sh -c "command -v $c; true")" ] || fail "o comando $c não deveria existir mais"
done
docker exec aic-srv sh -c "test ! -e /usr/local/bin/ai-cluster && test ! -e /usr/local/bin/aic-setup" || fail "o instalador não deveria criar os nomes antigos"
docker exec aic-srv aic status >/dev/null 2>&1 && fail "aic status (o antigo, de nós e modelo) não deveria existir"
OUT=$(docker exec aic-wk aic worker role 2>&1); RC=$?
[ "$RC" = 1 ] || fail "aic worker role no Linux comum deveria sair com 1 (saiu $RC)"
echo "$OUT" | grep -q "disponível apenas no AI Cluster OS" || fail "aic worker role no Linux comum deveria dizer que é só do AI Cluster OS"
echo "OK I2) comandos antigos não existem e aic worker role é recusado no Linux comum"
echo "=== instalador guiado OK ==="
