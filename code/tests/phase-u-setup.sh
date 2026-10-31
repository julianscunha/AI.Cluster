#!/usr/bin/env bash
# Instalador guiado (install.sh + setup + o comando `aic`): servidor, worker e gateway noutro computador,
# em contêineres Debian 12 (sem systemd, e com um systemd de mentira onde o teste diz), com um espelho local
# no lugar da release do GitHub.
#
#   bash code/scripts/package-release.sh ../../temp/rel11 && bash code/tests/phase-u-setup.sh ../../temp/rel11
#
# Critérios verificados:
#   A) install.sh instala o servidor (controller + gateway) baixando só os pacotes desse papel, com o
#      cabeçalho do instalador e o aviso de que, sem systemd, nada volta sozinho depois de um reinício;
#   B) o convite entrega o pacote do worker pelo processador e o nó fica READY e aprovado;
#   B1) um worker com systemd de mentira: `enable` é chamado e a unit tem Restart=always;
#   B2) o /wait do convite apresenta um computador com Linux para instalação e o operador o vê;
#   B2b) `aic worker change` muda um nó de controller pelo convite (token, CA, certificado novo; a troca só
#        se dá por feita com o registro novo no log; falha, ou worker que não registra, volta tudo);
#        `--token-file` troca controller e token (espaços e quebras saem; arquivo ausente, vazio ou legível
#        por outros é recusado sem mexer no nó);
#   B3) o worker exige o código do convite (8 caracteres) e o convite trava com chutes demais, inclusive com
#        HEAD e com caminho codificado (/%6Aoin-X); quem não chutou continua sendo atendido;
#   C) o controller sozinho e o gateway em outro computador sobem (o gateway com o controller dele);
#   C2) aic update apply no controller (com --force): troca o processo (pid novo), guarda o anterior e mantém
#        token, certificados e config; arquivo de config sem a quebra final ganha a chave nova e mantém a permissão;
#   C2b) aic update apply num gateway sozinho (sem server-ip) funciona e acrescenta o endereço dele;
#   D) processador errado, gateway em ARM de 32 bits e ARM de 64 bits são recusados com mensagem;
#      --lang fr é recusado e --mode dedicated (alias em inglês) funciona;
#   E) em inglês (--lang en): banner e recusa do install.sh, idioma salvo lido pelo setup e o
#      convite gera join e wait em inglês;
#   G) controller e gateway com systemd de mentira: habilitados no boot, Restart=always, WantedBy e
#      InaccessiblePaths (o gateway não lê controller.env nem cli.env; o controller não lê gateway.env);
#   G2) rollback: (a) o restart falha uma vez: binários, config e release voltam, e o serviço antigo sobe;
#        (b) o binário novo sai na hora: o update desfaz tudo, volta ao antigo e sai com erro;
#   H) aic update no worker Linux (--force): o pid muda, o registro novo aparece no log e os anteriores ficam guardados;
#   H1) rollback do worker: o worker novo sai na hora (release ruim): o update sai com erro, volta o binário, a
#        release e o .prev, e o worker ANTIGO roda de novo e registra (pid e registro conferidos);
#   C3) servidor no layout antigo (ai-cluster, aic-setup, sem /opt/ai-cluster/libexec nem aic): o setup do
#        tools.tar.gz atualiza; aic funciona, os nomes antigos somem, token e certificados ficam, config nova entra;
#   C4) assinatura da release (ADR-025): servidor sem a chave recusa com o caminho de migração; adulterada,
#        sem assinatura, de outra chave ou com pacote trocado recusa sem mexer em nada (estado conferido);
#        o install.sh recusa antes de instalar; a versão nova assinada atualiza; a mais antiga pede --force;
#   H3) worker Linux (ADR-025): --verify confere sem gravar; pedido recusado não aplica e deixa o motivo;
#        pedido assinado é gravado, o ai-cluster-update (root) aplica e o pid muda;
#   I) cabeçalho com o nome do componente, cor num terminal, comandos antigos inexistentes e
#      `aic worker role` recusado no Linux comum.
set -uo pipefail
export MSYS_NO_PATHCONV=1
DISTU=$(cd "${1:-dist/release}" && pwd)
DIST=$(cd "${1:-dist/release}" && pwd -W 2>/dev/null || pwd)
NET=aic-st
BASE=http://aic-mirror:9000
BASE_BAD=http://aic-mirror-bad:9001
TMPD=$(mktemp -d "${AIC_TMP:-/d/temp}/aic-setup.XXXXXX")   # caminho ASCII: o Docker não aceita acento no -v
BADDIST=$TMPD/bad; mkdir -p "$BADDIST"; cp -r "$DISTU"/. "$BADDIST"/
teardown() { docker rm -f aic-mirror aic-mirror-bad aic-srv aic-wk aic-wj aic-ctl aic-gw aic-sdc aic-sdw aic-old >/dev/null 2>&1; docker network rm $NET >/dev/null 2>&1; }
fail() { echo "FALHA: $1"; docker exec aic-srv tail -5 /tmp/invite.log 2>&1; teardown; exit 1; }
trap 'teardown; rm -rf "$TMPD"' EXIT
teardown; docker network create $NET >/dev/null

# espera_ate <segundos> <comando...>: repete até dar certo (ou estoura o prazo). Substitui os `sleep` fixos.
espera_ate() { local lim=$1; shift; local i=0; while [ "$i" -lt "$lim" ]; do "$@" >/dev/null 2>&1 && return 0; sleep 1; i=$((i + 1)); done; return 1; }
# falha_com <motivo> <comando...>: o comando tem de falhar E a saída tem de trazer o motivo esperado.
falha_com() {
  local motivo=$1; shift; local out rc
  out=$("$@" 2>&1); rc=$?
  [ "$rc" -ne 0 ] || fail "devia falhar: $*"
  case "$out" in *"$motivo"*) ;; *) echo "$out" | tail -4; fail "a falha não trouxe o motivo \"$motivo\"" ;; esac
}

# Imagem base com as dependências do instalador (construída uma vez; o cache do Docker a reaproveita).
docker build -q -t aic-test-base - >/dev/null <<'DOCKERFILE' || { echo "FALHA: imagem base"; exit 1; }
FROM debian:12
RUN apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq openssl libgomp1 iproute2 python3 curl wget procps >/dev/null && rm -rf /var/lib/apt/lists/*
DOCKERFILE
# Release "ruim" para os testes de rollback: o controller e o worker que saem na hora (o resto é igual). Feita
# num contêiner Linux, para o modo de execução do binário e o SHA-256 saírem certos.
docker run --rm -v "$(cd "$BADDIST" && pwd -W):/b" aic-test-base sh -c '
  cd /b && for p in controller:ai-cluster-controller worker:ai-cluster-worker; do
    a=${p%%:*}-linux-x86_64.tar.gz; bin=${p#*:}; rm -rf /w && mkdir /w && tar -xzf "$a" -C /w &&
    printf "#!/bin/sh\nexit 1\n" > /w/$bin && chmod 755 /w/$bin &&
    tar --owner=0 --group=0 -C /w -czf "$a" . || exit 1
  done && rm -rf /w && rm -f SHA256SUMS &&
  for f in install.sh VERSION *.tar.gz; do printf "%s  %s\n" "$(sha256sum $f | cut -d" " -f1)" "$f"; done > SHA256SUMS' \
  || { echo "FALHA: não consegui montar a release ruim"; exit 1; }
# Assinatura (ADR-025): a chave de Linux comum do operador (~/.aic/linux.key, a mesma do package-release).
LKEY=${AIC_LINUX_KEY:-$HOME/.aic/linux.key}
win() { cygpath -m "$1" 2>/dev/null || echo "$1"; }   # o openssl do Windows não lê caminhos /d/…
assina() { # assina <pasta> [chave]: refaz o SHA256SUMS.sig da pasta
  openssl pkeyutl -sign -inkey "$(win "${2:-$LKEY}")" -rawin -in "$(win "$1/SHA256SUMS")" -out "$(win "$TMPD/sig.bin")" \
    && printf '%s' "$(base64 -w0 < "$TMPD/sig.bin")" > "$1/SHA256SUMS.sig"
}
# A release ruim sai na hora, mas é assinada: o que os testes de rollback miram é o processo, não a assinatura.
rm -f "$BADDIST/SHA256SUMS.sig"; assina "$BADDIST" || { echo "FALHA: não assinei a release ruim"; exit 1; }
# Variantes com um defeito cada (pastas no espelho ruim, com os arquivos da release boa por ligação física):
#   sig-adulterada: o SHA256SUMS mudou depois de assinado; sig-semsig: sem SHA256SUMS.sig; sig-chave: assinada
#   por outra chave; sig-pacote: um pacote não bate com o SHA256SUMS; sig-novo: versão nova (0.2.1), válida.
boa() { mkdir -p "$BADDIST/$1"; cp -l "$DISTU"/*.tar.gz "$DISTU"/install.sh "$DISTU"/VERSION "$DISTU"/SHA256SUMS "$DISTU"/SHA256SUMS.sig "$BADDIST/$1/"; }
boa sig-adulterada; rm -f "$BADDIST/sig-adulterada/SHA256SUMS"; { cat "$DISTU/SHA256SUMS"; echo "# adulterado"; } > "$BADDIST/sig-adulterada/SHA256SUMS"
boa sig-semsig; rm -f "$BADDIST/sig-semsig/SHA256SUMS.sig"
boa sig-chave; openssl genpkey -algorithm ed25519 -out "$(win "$TMPD/outra.key")" 2>/dev/null
rm -f "$BADDIST/sig-chave/SHA256SUMS.sig"; assina "$BADDIST/sig-chave" "$TMPD/outra.key" || { echo "FALHA: chave descartável"; exit 1; }
boa sig-pacote; rm -f "$BADDIST/sig-pacote/controller-linux-x86_64.tar.gz"; { cat "$DISTU/controller-linux-x86_64.tar.gz"; echo x; } > "$BADDIST/sig-pacote/controller-linux-x86_64.tar.gz"
mkdir -p "$BADDIST/sig-novo"; cp -l "$DISTU"/*.tar.gz "$DISTU"/install.sh "$BADDIST/sig-novo/"; printf '0.2.1\n' > "$BADDIST/sig-novo/VERSION"
(cd "$BADDIST/sig-novo" && for f in install.sh VERSION *.tar.gz; do printf '%s  %s\n' "$(sha256sum "$f" | cut -d' ' -f1)" "$f"; done > SHA256SUMS)
assina "$BADDIST/sig-novo" || { echo "FALHA: não assinei a versão nova"; exit 1; }
docker run -d --name aic-mirror --network $NET -v "$DIST:/r:ro" -w /r aic-test-base python3 -m http.server 9000 >/dev/null
docker run -d --name aic-mirror-bad --network $NET -v "$(cd "$BADDIST" && pwd -W):/r:ro" -w /r aic-test-base python3 -m http.server 9001 >/dev/null
for n in aic-srv aic-wk aic-ctl aic-gw aic-sdc aic-sdw; do docker run -d --name $n --network $NET aic-test-base sleep 900 >/dev/null; done
for n in aic-srv aic-wk aic-ctl aic-gw aic-sdc aic-sdw; do
  espera_ate 60 docker exec $n curl -fsS $BASE/install.sh -o /tmp/install.sh || fail "sem install.sh no espelho ($n)"
done
inst() { n=$1; shift; docker exec $n sh -c "AIC_BASE=$BASE sh /tmp/install.sh --mode linux --kind conteiner --yes $*" 2>&1; }
# systemctl e journalctl de mentira: registram as chamadas, e o start/restart sobe o ExecStart da unit com o
# EnvironmentFile dela (o processo é o de verdade). O journalctl mostra o registro que o serviço gravou.
# Um `restart` com /run/fake-fail-restart falha uma vez (e apaga o marcador): simula um restart que não sai.
fake_systemd() {
  docker exec -i "$1" sh -c 'mkdir -p /run/systemd/system /etc/systemd/system && cat > /usr/local/bin/systemctl && chmod 755 /usr/local/bin/systemctl' <<'FAKE'
#!/bin/sh
echo "$*" >> /var/log/fake-systemctl.log
mkdir -p /run/fake-units /var/log/fake-units
for svc in "$@"; do :; done
case "$1" in
is-active) pid=$(cat /run/fake-units/$svc.pid 2>/dev/null); [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; exit $? ;;
start|restart)
  if [ -f /run/fake-fail-restart ]; then rm -f /run/fake-fail-restart; exit 1; fi
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
pid_de() { docker exec "$1" cat "$2" 2>/dev/null; }   # pid_de <contêiner> <arquivo-do-pid>

echo "=== A) servidor completo baixa só controller, gateway e tools ==="
OUT=$(inst aic-srv --role server --arch x86_64) || { echo "$OUT"; fail "install.sh server"; }
echo "$OUT" | grep -q "✔ gateway-linux-x86_64" || fail "o gateway deveria ter sido baixado"
echo "$OUT" | grep -q "worker-linux" && fail "o servidor não deveria baixar o worker"
docker exec aic-srv sh -c 'grep -q "^AI_CLUSTER_GATEWAY_URL=http://$(cat /etc/ai-cluster/server-ip):8080$" /etc/ai-cluster/gateway.env' || fail "o gateway do servidor deveria anunciar http://<ip>:8080 (e não 0.0.0.0)"
echo "$OUT" | grep -q "^Real.AI.Cluster instalador - " || fail "o instalador deveria abrir com o cabeçalho dele"
echo "$OUT" | grep -q "não voltam sozinhos" || fail "sem systemd o instalador deveria avisar que nada volta sozinho"
docker exec aic-srv sh -c 'test -x /opt/ai-cluster/libexec/setup && test -x /usr/local/bin/aic && test -x /opt/ai-cluster/bin/aic' || fail "o setup e o aic deveriam ficar em /opt/ai-cluster"
# A chave de release é pinada na instalação (ADR-025): a do pacote tools, que é a do repositório.
[ "$(docker exec aic-srv sha256sum /etc/ai-cluster/release.pub | cut -d' ' -f1)" = "$(sha256sum "$(dirname "$0")/../os/linux.pub" | cut -d' ' -f1)" ] \
  || fail "a instalação deveria gravar /etc/ai-cluster/release.pub (a chave de Linux comum)"
[ -s "$DISTU/SHA256SUMS.sig" ] || fail "a release deveria trazer SHA256SUMS.sig"
echo "OK: servidor instalado, com a chave de release pinada"

echo "=== B) o convite entrega o worker e o nó fica READY e aprovado ==="
docker exec -d aic-srv sh -c "aic cluster invite --minutes 4 > /tmp/invite.log 2>&1"
espera_ate 40 docker exec aic-srv grep -q -- '--code' /tmp/invite.log || fail "o convite não mostrou o código"
SIP=$(docker exec aic-srv cat /etc/ai-cluster/server-ip)
WCODE=$(docker exec aic-srv sh -c "grep -o -- '--code [A-Za-z0-9]*' /tmp/invite.log | head -n 1 | cut -d' ' -f2")
[ ${#WCODE} -eq 8 ] || fail "o convite deveria mostrar um código de 8 caracteres (veio: $WCODE)"
docker exec aic-srv grep -q "^Real.AI.Cluster convite " /tmp/invite.log || fail "o convite deveria abrir com o cabeçalho dele"
# O worker precisa do código: sem ele o instalador recusa (sem terminal para perguntar), o /join não existe e o /sign nega.
falha_com "sem terminal para perguntar" inst aic-wk --role worker --arch x86_64 --server "$SIP"
[ "$(docker exec aic-wk curl -s -o /dev/null -w '%{http_code}' http://$SIP:8099/join)" = 404 ] || fail "o /join sem código não deveria existir"
[ "$(docker exec aic-wk curl -s -o /dev/null -w '%{http_code}' -X POST --data x "http://$SIP:8099/sign?name=intruso&code=ZZZZZZZZ")" = 403 ] || fail "o /sign com código errado deveria dar 403"
OUT=$(docker exec aic-wk sh -c "AIC_CODE=$(echo $WCODE | tr A-Z a-z) AIC_BASE=$BASE sh /tmp/install.sh --mode linux --kind conteiner --yes --role worker --arch x86_64 --server $SIP" 2>&1) || { echo "$OUT" | tail -5; fail "o worker não entrou"; }
espera_ate 40 docker exec aic-srv sh -c "aic cluster nodes 2>/dev/null | grep -E 'READY.*aprovado'" || fail "o nó não ficou READY e aprovado"
echo "OK: worker READY e aprovado"
# Certificado por nó: a chave nasceu no worker (o servidor não tem chave de worker) e o SAN leva o nome do nó.
docker exec aic-srv test ! -e /etc/ai-cluster/pki/worker.key || fail "o servidor não deveria guardar chave de worker"
WNAME=$(docker exec aic-wk hostname | sed 's/[^A-Za-z0-9.-]/-/g' | cut -c1-40)
docker exec aic-wk sh -c "openssl x509 -in /etc/ai-cluster/rpc-tls/worker.crt -noout -ext subjectAltName | grep -q 'DNS:aic-worker-$WNAME' && openssl verify -CAfile /etc/ai-cluster/rpc-tls/ca.crt /etc/ai-cluster/rpc-tls/worker.crt >/dev/null"   || fail "o certificado do worker deveria ter o SAN aic-worker-$WNAME e conferir com a CA"
docker exec aic-srv grep -q 'AI_CLUSTER_LLAMA_RPC_SERVER_NAME=aic-worker-{node_id}' /etc/ai-cluster/gateway.env || fail "o gateway deveria esperar o nome por nó"
echo "OK: certificado por nó"
# O worker entrou pelo join novo: ele grava a release (origem da atualização) e leva o aic e o setup.
docker exec aic-wk sh -c 'grep -q "^ROLE=worker$" /etc/ai-cluster/release && grep -q "^BASE=http" /etc/ai-cluster/release && grep -q "^SUMS=[0-9a-f]" /etc/ai-cluster/release && test -x /opt/ai-cluster/libexec/setup && test -x /opt/ai-cluster/bin/aic && test -e /etc/ai-cluster/.installed' || fail "o join deveria gravar /etc/ai-cluster/release e instalar o aic e o setup"
echo "OK: join grava a release e instala aic e setup"

# B0) ADR-025: o worker que entra pelo convite direto (curl | sh, sem o install.sh) recebe a chave de release do
# servidor e pina; o pedido assinado passa pelo --verify dele.
docker run -d --name aic-wj --network $NET aic-test-base sleep 900 >/dev/null
OUT=$(docker exec aic-wj sh -c "curl -fsSL http://$SIP:8099/join-$WCODE | sh" 2>&1) || { echo "$OUT" | tail -6; fail "o worker que entra por curl | sh não entrou"; }
[ "$(docker exec aic-wj sha256sum /etc/ai-cluster/release.pub | cut -d' ' -f1)" = "$(sha256sum "$(dirname "$0")/../os/linux.pub" | cut -d' ' -f1)" ] \
  || fail "o worker do convite deveria pinar a chave de release (/etc/ai-cluster/release.pub)"
docker exec aic-wj runuser -u ai-cluster -- /opt/ai-cluster/bin/aic-update-linux --verify "$(cat "$DISTU/VERSION")" "$BASE" x x "$(cat "$DISTU/SHA256SUMS.sig")" >/dev/null 2>&1 \
  || fail "o worker do convite deveria aceitar o pedido assinado (--verify)"
echo "OK B0: worker do convite (curl | sh) pina a chave de release e o --verify aceita o pedido assinado"

echo "=== B1) worker com systemd de mentira: habilitado no boot, com Restart=always ==="
fake_systemd aic-sdw
falha_com "sem terminal para perguntar" inst aic-sdw --role worker --arch x86_64 --server "$SIP"
OUT=$(docker exec aic-sdw sh -c "AIC_CODE=$(echo $WCODE | tr A-Z a-z) AIC_BASE=$BASE sh /tmp/install.sh --mode linux --kind conteiner --yes --role worker --arch x86_64 --server $SIP" 2>&1) || { echo "$OUT" | tail -5; fail "o worker com systemd não entrou"; }
docker exec aic-sdw grep -qx 'enable ai-cluster-worker' /var/log/fake-systemctl.log || fail "o worker deveria ser habilitado no boot (systemctl enable)"
docker exec aic-sdw grep -qx '^Restart=always$' /etc/systemd/system/ai-cluster-worker.service || fail "a unit do worker deveria ter Restart=always"
docker exec aic-sdw grep -qx '^WantedBy=multi-user.target$' /etc/systemd/system/ai-cluster-worker.service || fail "a unit do worker deveria ser WantedBy=multi-user.target"
docker exec aic-sdw sh -c 'kill -0 $(cat /run/fake-units/ai-cluster-worker.pid)' || fail "o worker com systemd deveria estar rodando"
# Com systemd, o worker se declara atualizável (ADR-025) e o unit que aplica o pedido fica habilitado.
docker exec aic-sdw sh -c 'grep -qx "AI_CLUSTER_UPDATE_CMD=/opt/ai-cluster/bin/aic-update-linux" /opt/ai-cluster/worker.env && grep -qx "AI_CLUSTER_UPDATE_KIND=linux" /opt/ai-cluster/worker.env' \
  || fail "com systemd, o worker deveria se declarar atualizável (AI_CLUSTER_UPDATE_CMD)"
docker exec aic-sdw grep -qx 'PathExists=/opt/ai-cluster/state/update-request' /etc/systemd/system/ai-cluster-update.path || fail "o unit ai-cluster-update.path deveria esperar o pedido"
docker exec aic-sdw grep -qx 'enable --now ai-cluster-update.path' /var/log/fake-systemctl.log || fail "o path do update deveria ser habilitado no boot"
echo "OK B1: worker habilitado no boot e unit com Restart=always"

echo "=== B2) quem já tem Linux se apresenta pelo /wait e aparece para aprovar ==="
# O convite que está aberto também abriu a porta de espera; o script baixa só o aic-wait e o roda.
docker exec aic-wk sh -c "curl -fsSL http://$SIP:8099/wait -o /tmp/wait.sh" || fail "o servidor não serviu o /wait"
docker exec -d aic-wk sh -c "timeout 40 sh /tmp/wait.sh > /tmp/wait.log 2>&1"
espera_ate 40 docker exec aic-srv sh -c "aic cluster install list 2>/dev/null | grep -q 'fingerprint=.* aguardando'" \
  || fail "o aparelho não apareceu em install list: $(docker exec aic-wk tail -5 /tmp/wait.log)"
docker exec aic-wk grep -q "Impressão digital" /tmp/wait.log || fail "a tela de espera não mostrou a impressão digital"
espera_ate 30 docker exec aic-srv grep -q "quer instalar" /tmp/invite.log || fail "o convite deveria avisar o operador do aparelho"
echo "OK: aparelho aguardando aprovação"

# `aic worker change` (AI Cluster OS): muda um nó de controller pelo convite, sem regravar a imagem. O nó de
# mentira usa pastas e um rc-service de mentira (que registra no log do worker quando sobe); o resto
# (wget, openssl, convite) é de verdade. AIC_TESTING=1 libera os caminhos de teste; AIC_ASSUME_YES=1 pula a pergunta.
AICDIR=$(cd "$(dirname "$0")/../os/image/overlay/usr/libexec/aic" && pwd -W 2>/dev/null || pwd)
docker cp "$AICDIR/change" aic-wk:/tmp/aic-change >/dev/null || fail "docker cp do change"
cat > "$TMPD/rc" <<'RC'
#!/bin/sh
# rc-service de mentira: anota a chamada; `start` falha se /tmp/fd/fail-start existe e, se não existe
# /tmp/fd/no-register, grava o registro do worker no log de teste (como o worker de verdade faz).
echo "$@" >> /tmp/fd/rc.log
if [ "$1 $2" = "ai-cluster-worker start" ]; then
  if [ -f /tmp/fd/fail-start ]; then rm -f /tmp/fd/fail-start; exit 1; fi
  [ -f /tmp/fd/no-register ] || echo "registrado node_id=nodechange aceito=true cluster_id=x controller=y" >> /tmp/fd/worker.log
fi
exit 0
RC
docker exec aic-wk sh -c "mkdir -p /tmp/fd/state /tmp/fd/rpc-tls && printf 'AI_CLUSTER_CONTROLLER=http://velho:50051\nAI_CLUSTER_JOIN_TOKEN=velho\nAI_CLUSTER_NODE_ID=nodechange\nAI_CLUSTER_INSECURE=1\n' > /tmp/fd/worker.env && echo cred > /tmp/fd/state/credential && touch /tmp/fd/worker.log"
docker cp "$(cd "$TMPD" && pwd -W)/rc" aic-wk:/tmp/fd/rc >/dev/null || fail "docker cp do rc de mentira"
docker exec aic-wk chmod +x /tmp/fd/rc || fail "chmod do rc de mentira"
CH="AIC_TESTING=1 AIC_ASSUME_YES=1 AIC_DATA=/tmp/fd AIC_RC=/tmp/fd/rc AIC_WORKER_LOG=/tmp/fd/worker.log sh /tmp/aic-change"
# Sem terminal e sem --yes, nada muda: a pergunta é obrigatória.
falha_com "sem terminal para confirmar" docker exec aic-wk sh -c "AIC_TESTING=1 AIC_DATA=/tmp/fd AIC_RC=/tmp/fd/rc sh /tmp/aic-change --controller http://192.168.1.25:50051 --token novo"
docker exec aic-wk grep -q 'JOIN_TOKEN=velho' /tmp/fd/worker.env || fail "sem confirmação o nó não pode mudar"
# Código errado: falha com o motivo e o nó continua como estava.
falha_com "o convite nao respondeu" docker exec aic-wk sh -c "$CH --server $SIP:8099 --code ZZZZZZZZ"
docker exec aic-wk grep -q 'JOIN_TOKEN=velho' /tmp/fd/worker.env || fail "aic change com falha não pode mexer no nó"
docker exec aic-wk sh -c "$CH --server $SIP:8099 --code $(echo $WCODE | tr A-Z a-z) --controller http://192.168.1.25:50051" >/dev/null || fail "aic change pelo convite"
NEWTOK=$(docker exec aic-srv sed -n 's/^AI_CLUSTER_JOIN_TOKEN=//p' /etc/ai-cluster/controller.env)
docker exec aic-wk grep -q "AI_CLUSTER_JOIN_TOKEN=$NEWTOK" /tmp/fd/worker.env || fail "o token novo deveria estar no worker.env"
docker exec aic-wk grep -q 'AI_CLUSTER_CONTROLLER=http://192.168.1.25:50051' /tmp/fd/worker.env || fail "o --controller deveria valer"
docker exec aic-wk grep -q 'AI_CLUSTER_INSECURE=1' /tmp/fd/worker.env || fail "o resto do worker.env deveria ficar"
docker exec aic-wk sh -c "test ! -e /tmp/fd/state/credential && openssl verify -CAfile /tmp/fd/rpc-tls/ca.crt /tmp/fd/rpc-tls/worker.crt && openssl x509 -in /tmp/fd/rpc-tls/worker.crt -noout -ext subjectAltName | grep -q 'aic-worker-nodechange' && grep -q stop /tmp/fd/rc.log && grep -q start /tmp/fd/rc.log" >/dev/null || fail "aic change deveria trocar credencial e certificado e reiniciar o worker"
[ "$(docker exec aic-wk stat -c %a /tmp/fd/.change-backup)" = 700 ] || fail "o backup do change deveria ser 700"
docker exec aic-wk sh -c 'test -z "$(find /tmp/fd/.change-backup -perm /077)"' || fail "os arquivos do backup deveriam ser só do root"
# Token por arquivo: espaços e quebras saem; a recusa (ausente, vazio, legível por outros, junto de --token)
# não mexe no nó; o arquivo certo troca controller e token, e o token nunca aparece na saída.
docker exec aic-wk sh -c "printf '  tokenDoArquivo1 \n\n' > /tmp/fd/tok-ok && chmod 600 /tmp/fd/tok-ok && printf 'outro\n' > /tmp/fd/tok-aberto && chmod 644 /tmp/fd/tok-aberto && : > /tmp/fd/tok-vazio && chmod 600 /tmp/fd/tok-vazio" || fail "não preparei os arquivos de token"
ANTES_ARQ=$(docker exec aic-wk cat /tmp/fd/worker.env)
falha_com "ausente ou sem leitura" docker exec aic-wk sh -c "$CH --controller http://192.168.1.28:50051 --token-file /tmp/fd/nao-existe"
falha_com "legivel por outros" docker exec aic-wk sh -c "$CH --controller http://192.168.1.28:50051 --token-file /tmp/fd/tok-aberto"
falha_com "esta vazio" docker exec aic-wk sh -c "$CH --controller http://192.168.1.28:50051 --token-file /tmp/fd/tok-vazio"
falha_com "nao os dois" docker exec aic-wk sh -c "$CH --controller http://192.168.1.28:50051 --token-file /tmp/fd/tok-ok --token x"
[ "$(docker exec aic-wk cat /tmp/fd/worker.env)" = "$ANTES_ARQ" ] || fail "token por arquivo recusado não pode mexer no worker.env"
OUT_ARQ=$(docker exec aic-wk sh -c "$CH --controller http://192.168.1.28:50051 --token-file /tmp/fd/tok-ok" 2>&1) || { echo "$OUT_ARQ" | tail -4; fail "aic change com --token-file"; }
docker exec aic-wk grep -qx 'AI_CLUSTER_JOIN_TOKEN=tokenDoArquivo1' /tmp/fd/worker.env || fail "o token do arquivo deveria entrar no worker.env, sem espaços nem quebra"
docker exec aic-wk grep -qx 'AI_CLUSTER_CONTROLLER=http://192.168.1.28:50051' /tmp/fd/worker.env || fail "--controller com --token-file deveria valer"
echo "$OUT_ARQ" | grep -q tokenDoArquivo1 && fail "o token não pode aparecer na saída do aic worker change"
echo "OK: --token-file troca controller e token; recusas não mexem no nó e o token não aparece"
BEFORE=$(docker exec aic-wk cat /tmp/fd/worker.env)
# Worker que não sobe: volta o que estava (token, CA e certificado) e sai com erro.
docker exec aic-wk touch /tmp/fd/fail-start
falha_com "o worker nao subiu" docker exec aic-wk sh -c "$CH --controller http://192.168.1.26:50051 --token outro"
docker exec aic-wk rm -f /tmp/fd/fail-start
[ "$(docker exec aic-wk cat /tmp/fd/worker.env)" = "$BEFORE" ] || fail "com o worker que não subiu, o worker.env deveria voltar ao que era"
# Worker que sobe mas não se registra no novo controller em 30 s: também não se dá por feito.
docker exec aic-wk touch /tmp/fd/no-register
falha_com "nao se registrou" docker exec aic-wk sh -c "$CH --controller http://192.168.1.27:50051 --token outro"
docker exec aic-wk rm -f /tmp/fd/no-register
[ "$(docker exec aic-wk cat /tmp/fd/worker.env)" = "$BEFORE" ] || fail "sem o registro, o worker.env deveria voltar ao que era"
echo "OK: aic worker change troca de controller pelo convite, pede confirmação e desfaz se o worker não sobe ou não registra"

echo "=== B3) chutes travam o convite, inclusive HEAD e caminho codificado; quem não chutou segue atendido ==="
# 31 chutes: HEAD em caminho codificado (%6A = j) e um `/./` antes. Todos contam (GET ou HEAD, normalizados).
docker exec aic-wk sh -c "for i in \$(seq 16); do curl -s -o /dev/null -I 'http://$SIP:8099/%6Aoin-AAAAAAAA'; curl -s -o /dev/null -I 'http://$SIP:8099/./join-AAAAAAAA'; done"
[ "$(docker exec aic-wk curl -s -o /dev/null -w '%{http_code}' http://$SIP:8099/join-$WCODE)" = 429 ] || fail "o IP que chutou deveria levar 429 mesmo com o código certo"
[ "$(docker exec aic-sdc curl -s -o /dev/null -w '%{http_code}' http://$SIP:8099/join-$WCODE)" = 200 ] || fail "o código certo de outro IP deveria continuar sendo servido"
espera_ate 15 docker exec aic-srv grep -q "convite: .* travado por chutes" /tmp/invite.log || fail "o operador deveria ver o aviso de IP travado"
echo "OK: chutes travam o IP (HEAD e %6A contam), e outro IP segue atendido"

echo "=== C) controller sozinho e gateway noutro computador ==="
inst aic-ctl --role controller --arch x86_64 >/dev/null || fail "install.sh controller"
docker exec -d aic-ctl sh -c "aic cluster invite --minutes 4 > /tmp/invite.log 2>&1"
espera_ate 40 docker exec aic-ctl grep -q -- '--code' /tmp/invite.log || fail "o convite do controller não mostrou o código"
CIP=$(docker exec aic-ctl cat /etc/ai-cluster/server-ip)
GCODE=$(docker exec aic-ctl sh -c "grep -o -- '--code [A-Za-z0-9]*' /tmp/invite.log | head -n 1 | cut -d' ' -f2")
[ -n "$GCODE" ] || fail "o convite deveria mostrar o código do gateway"
falha_com "sem terminal para perguntar" inst aic-gw --role gateway --arch x86_64 --server "$CIP"
docker exec aic-gw sh -c "AIC_CODE=$GCODE AIC_BASE=$BASE sh /tmp/install.sh --mode linux --kind conteiner --yes --role gateway --arch x86_64 --server $CIP" >/dev/null 2>&1 || fail "install.sh gateway"
espera_ate 40 docker exec aic-gw sh -c "ss -ltn | grep -q ':8080 '" || fail "o gateway não abriu a porta 8080"
espera_ate 40 docker exec aic-gw curl -fsS http://127.0.0.1:8080/health || fail "o gateway deveria responder ao /health"
docker exec aic-gw grep -q "controller http://$CIP:50051" /var/log/ai-cluster/gateway.log || fail "o gateway deveria apontar para o controller"
docker exec aic-gw sh -c 'grep -q "^AI_CLUSTER_GATEWAY_URL=http://$(hostname -i | cut -d" " -f1):8080$" /etc/ai-cluster/gateway.env' || fail "o gateway noutro computador deveria anunciar o IP dele"
echo "OK: controller e gateway em computadores separados"

echo "=== C2) aic update apply no controller (--force): pid novo, config e token mantidos ==="
TOK=$(docker exec aic-ctl sed -n 's/^AI_CLUSTER_JOIN_TOKEN=//p' /etc/ai-cluster/controller.env)
OUT=$(docker exec -e AIC_LANG=pt aic-ctl aic update apply --yes 2>&1)
echo "$OUT" | grep -q "já está na versão mais recente" || { echo "$OUT" | tail -5; fail "sem novidade, o update deveria dizer que já está na versão mais recente"; }
# Simula uma instalação antiga: sem o resumo da release e sem a porta de espera; a config sem a quebra final.
docker exec aic-ctl sh -c "sed -i '/^SUMS=/d' /etc/ai-cluster/release && sed -i '/^AI_CLUSTER_ENROLL_ADDR=/d' /etc/ai-cluster/controller.env && t=\$(cat /etc/ai-cluster/controller.env) && printf '%s' \"\$t\" > /etc/ai-cluster/controller.env" \
  || fail "não consegui simular a instalação antiga"
docker exec aic-ctl sh -c "! grep -q '^SUMS=' /etc/ai-cluster/release && ! grep -q '^AI_CLUSTER_ENROLL_ADDR=' /etc/ai-cluster/controller.env && [ -n \"\$(tail -c 1 /etc/ai-cluster/controller.env)\" ]" \
  || fail "a simulação deveria tirar SUMS e ENROLL_ADDR e deixar a config sem a quebra final"
docker exec aic-ctl stat -c %a /etc/ai-cluster/controller.env | grep -qx 640 || fail "a permissão do controller.env deveria ser 640 antes do update"
PID0=$(pid_de aic-ctl /var/lib/ai-cluster/state/controller.pid)
OUT=$(docker exec aic-ctl aic update apply --force --yes 2>&1) || { echo "$OUT" | tail -8; fail "aic update apply --force"; }
echo "$OUT" | grep -q "Atualizado para" || { echo "$OUT" | tail -8; fail "o update deveria terminar com 'Atualizado para'"; }
PID1=$(pid_de aic-ctl /var/lib/ai-cluster/state/controller.pid)
[ -n "$PID1" ] && [ "$PID1" != "$PID0" ] || fail "o update deveria reiniciar o controller (pid antes=$PID0 depois=$PID1)"
docker exec aic-ctl kill -0 "$PID1" || fail "o controller novo deveria estar vivo"
docker exec aic-ctl aic cluster nodes >/dev/null 2>&1 || fail "o controller deveria responder depois do update"
[ "$(docker exec aic-ctl sed -n 's/^AI_CLUSTER_JOIN_TOKEN=//p' /etc/ai-cluster/controller.env)" = "$TOK" ] || fail "o update não pode trocar o token de join"
docker exec aic-ctl grep -qx '^AI_CLUSTER_ENROLL_ADDR=.*:50052$' /etc/ai-cluster/controller.env || fail "o update deveria acrescentar a config nova que faltava, em linha própria"
docker exec aic-ctl stat -c %a /etc/ai-cluster/controller.env | grep -qx 640 || fail "o update não pode mudar a permissão do controller.env"
docker exec aic-ctl test -x /opt/ai-cluster/bin/.prev/ai-cluster-controller || fail "o update deveria guardar os binários anteriores"
docker exec aic-ctl grep -q '^SUMS=' /etc/ai-cluster/release || fail "o update deveria gravar o resumo da release"
echo "OK: aic update apply no controller (pid novo, config e token mantidos)"

echo "=== C2b) aic update apply num gateway sozinho: funciona e acrescenta o endereço dele ==="
docker exec aic-gw sh -c "sed -i '/^AI_CLUSTER_GATEWAY_URL=/d' /etc/ai-cluster/gateway.env && sed -i '/^SUMS=/d' /etc/ai-cluster/release" || fail "não consegui simular o gateway antigo"
GPID0=$(pid_de aic-gw /var/lib/ai-cluster/state/gateway.pid)
OUT=$(docker exec aic-gw aic update apply --force --yes 2>&1) || { echo "$OUT" | tail -8; fail "aic update apply no gateway sozinho morreu"; }
echo "$OUT" | grep -q "Atualizado para" || { echo "$OUT" | tail -8; fail "o update do gateway deveria terminar com 'Atualizado para'"; }
GPID1=$(pid_de aic-gw /var/lib/ai-cluster/state/gateway.pid)
[ -n "$GPID1" ] && [ "$GPID1" != "$GPID0" ] || fail "o update deveria reiniciar o gateway (pid antes=$GPID0 depois=$GPID1)"
docker exec aic-gw sh -c 'grep -q "^AI_CLUSTER_GATEWAY_URL=http://$(hostname -i | cut -d" " -f1):8080$" /etc/ai-cluster/gateway.env' || fail "o gateway deveria voltar com o endereço dele (lan_ip)"
espera_ate 40 docker exec aic-gw curl -fsS http://127.0.0.1:8080/health || fail "o gateway deveria responder depois do update"
echo "OK: update do gateway sozinho"

echo "=== C3) servidor com o layout antigo (ai-cluster, aic-setup): o setup do tools.tar.gz migra e atualiza ==="
docker run -d --name aic-old --network $NET aic-test-base sleep 900 >/dev/null
espera_ate 60 docker exec aic-old curl -fsS $BASE/install.sh -o /tmp/install.sh || fail "sem install.sh no espelho (aic-old)"
inst aic-old --role controller --arch x86_64 >/dev/null || fail "install.sh controller (layout antigo)"
JOIN_C3=$(docker exec aic-old sed -n 's/^AI_CLUSTER_JOIN_TOKEN=//p' /etc/ai-cluster/controller.env)
PKI_C3=$(docker exec aic-old sh -c 'sha256sum /etc/ai-cluster/pki/* | sort')
# O layout antigo: sem libexec/setup, sem o wrapper `aic` e sem o binário `aic`; os nomes ai-cluster e
# aic-setup como arquivos simples; o release sem o resumo SUMS e sem a porta de espera (config antiga).
docker exec aic-old sh -c '
  rm -rf /opt/ai-cluster/libexec && rm -f /usr/local/bin/aic /opt/ai-cluster/bin/aic &&
  printf "#!/bin/sh\necho ai-cluster antigo\n" > /usr/local/bin/ai-cluster && cp /usr/local/bin/ai-cluster /usr/local/bin/aic-setup &&
  cp /usr/local/bin/ai-cluster /opt/ai-cluster/bin/ai-cluster && chmod 755 /usr/local/bin/ai-cluster /usr/local/bin/aic-setup /opt/ai-cluster/bin/ai-cluster &&
  sed -i "/^SUMS=/d" /etc/ai-cluster/release && sed -i "/^AI_CLUSTER_ENROLL_ADDR=/d" /etc/ai-cluster/controller.env' \
  || fail "não consegui montar o layout antigo"
docker exec aic-old sh -c "! command -v aic >/dev/null 2>&1 && test -x /usr/local/bin/ai-cluster && test -x /usr/local/bin/aic-setup" || fail "o layout antigo deveria estar montado"
docker exec aic-old sh -c "mkdir -p /tmp/tools && curl -fsS $BASE/tools.tar.gz -o /tmp/tools.tgz && tar -C /tmp/tools -xzf /tmp/tools.tgz && cp /tmp/tools/setup /tmp/setup" \
  || fail "não consegui o setup do tools.tar.gz"
# Antes, uma atualização que falha (release ruim: o controller sai na hora): volta tudo, e o layout antigo
# continua como estava, com ai-cluster e aic-setup funcionando (os nomes só saem depois que o novo sobe).
docker exec aic-old sed -i "s|^BASE=.*|BASE=$BASE_BAD|" /etc/ai-cluster/release || fail "não apontei o servidor antigo para a release ruim"
OUT=$(docker exec aic-old sh /tmp/setup update apply --force --yes 2>&1); RC=$?
[ "$RC" != 0 ] || { echo "$OUT" | tail -8; fail "a migração com release ruim deveria sair com erro"; }
echo "$OUT" | grep -q "voltou a rodar" || { echo "$OUT" | tail -8; fail "a migração desfeita deveria dizer que a versão anterior voltou"; }
docker exec aic-old sh -c 'test -x /usr/local/bin/ai-cluster && test -x /usr/local/bin/aic-setup && test -x /opt/ai-cluster/bin/ai-cluster' \
  || fail "a migração desfeita não pode tirar os nomes antigos"
docker exec aic-old sh -c '/usr/local/bin/ai-cluster >/dev/null && /usr/local/bin/aic-setup >/dev/null' || fail "os comandos antigos deveriam funcionar depois do rollback"
docker exec aic-old sh -c 'test ! -e /usr/local/bin/aic && test ! -e /opt/ai-cluster/libexec/setup' || fail "o rollback deveria tirar o aic novo (layout antigo)"
espera_ate 40 docker exec aic-old sh -c 'kill -0 $(cat /var/lib/ai-cluster/state/controller.pid)' || fail "o controller antigo deveria estar rodando depois do rollback"
[ "$(docker exec aic-old sed -n 's/^AI_CLUSTER_JOIN_TOKEN=//p' /etc/ai-cluster/controller.env)" = "$JOIN_C3" ] || fail "o rollback não pode trocar o token"
docker exec aic-old sed -i "s|^BASE=.*|BASE=$BASE|" /etc/ai-cluster/release || fail "não voltei o servidor antigo para a release boa"
echo "OK C3a) migração com release ruim: volta ao layout antigo, ai-cluster e aic-setup funcionam"
OUT=$(docker exec aic-old sh /tmp/setup update apply --force --yes 2>&1) || { echo "$OUT" | tail -12; fail "o setup do tools.tar.gz deveria atualizar o servidor antigo"; }
echo "$OUT" | grep -q "Atualizado para" || { echo "$OUT" | tail -8; fail "a migração deveria terminar com 'Atualizado para'"; }
docker exec aic-old sh -c 'test -x /usr/local/bin/aic && test -x /opt/ai-cluster/bin/aic && test -x /opt/ai-cluster/libexec/setup' || fail "a migração deveria pôr o aic e o setup novos"
docker exec aic-old aic version >/dev/null 2>&1 || fail "o aic deveria funcionar depois da migração"
docker exec aic-old sh -c 'test ! -e /usr/local/bin/ai-cluster && test ! -e /usr/local/bin/aic-setup && test ! -e /opt/ai-cluster/bin/ai-cluster' || fail "os nomes antigos (ai-cluster, aic-setup) deveriam sumir"
[ "$(docker exec aic-old sed -n 's/^AI_CLUSTER_JOIN_TOKEN=//p' /etc/ai-cluster/controller.env)" = "$JOIN_C3" ] || fail "a migração não pode trocar o token de join"
[ "$(docker exec aic-old sh -c 'sha256sum /etc/ai-cluster/pki/* | sort')" = "$PKI_C3" ] || fail "a migração não pode trocar os certificados"
docker exec aic-old grep -qx '^AI_CLUSTER_ENROLL_ADDR=.*:50052$' /etc/ai-cluster/controller.env || fail "a migração deveria acrescentar a config nova"
docker exec aic-old test -x /opt/ai-cluster/bin/.prev/ai-cluster-controller || fail "a migração deveria guardar o binário anterior em .prev"
espera_ate 40 docker exec aic-old aic cluster nodes || fail "o controller deveria responder depois da migração"
echo "OK C3) servidor antigo migrado pelo setup do tools.tar.gz: aic funciona, nomes antigos somem, token e certificados ficam"

echo "=== C4) assinatura da release (ADR-025): válida atualiza; adulterada, sem assinatura ou de outra chave recusa sem mexer em nada ==="
# Servidor sem a chave pinada (instalado antes da assinatura): recusa com o caminho de migração e nada muda.
docker exec aic-old mv /etc/ai-cluster/release.pub /tmp/release.pub.guardado || fail "não consegui tirar a chave do servidor antigo"
falha_com "anterior à assinatura" docker exec aic-old aic update status
falha_com "anterior à assinatura" docker exec aic-old aic update apply --yes
docker exec aic-old mv /tmp/release.pub.guardado /etc/ai-cluster/release.pub || fail "não devolvi a chave ao servidor"
estado() { docker exec aic-old sh -c 'sha256sum /opt/ai-cluster/bin/ai-cluster-controller /etc/ai-cluster/controller.env; grep -v ^BASE= /etc/ai-cluster/release | sha256sum; cat /var/lib/ai-cluster/state/controller.pid'; }   # sem a linha BASE: o próprio teste a troca
ANTES_C4=$(estado)
# A release de outra origem: a assinatura (ou o pacote) não confere; a recusa diz o motivo e nada é trocado.
falha_com "não confere com a chave" docker exec aic-old sh -c "sed -i 's|^BASE=.*|BASE=$BASE_BAD/sig-adulterada|' /etc/ai-cluster/release && aic update status"
for par in "sig-adulterada|não confere com a chave" "sig-semsig|não tem assinatura" "sig-chave|não confere com a chave" "sig-pacote|não confere com o SHA256SUMS"; do
  dir=${par%%|*}; motivo=${par#*|}
  docker exec aic-old sed -i "s|^BASE=.*|BASE=$BASE_BAD/$dir|" /etc/ai-cluster/release || fail "não apontei o servidor para $dir"
  falha_com "$motivo" docker exec aic-old aic update apply --force --yes
  [ "$(estado)" = "$ANTES_C4" ] || fail "com $dir recusada, nada pode mudar (binário, release, config ou pid)"
done
# O instalador também confere antes de instalar: a release adulterada nem chega ao setup.
OUT=$(docker exec aic-ctl sh -c "AIC_BASE=$BASE_BAD/sig-chave sh /tmp/install.sh --mode linux --kind conteiner --yes --role controller --arch x86_64" 2>&1); RC=$?
[ "$RC" != 0 ] && echo "$OUT" | grep -q "assinatura da release não confere" || { echo "$OUT" | tail -4; fail "install.sh deveria recusar a release de outra chave"; }
echo "OK C4a) recusas: assinatura adulterada, ausente, de outra chave e pacote trocado (nada mudou); install.sh recusa antes de instalar"
# A release válida (a mesma versão) não atualiza; uma versão nova assinada atualiza; uma mais antiga é recusada sem --force.
docker exec aic-old sed -i "s|^BASE=.*|BASE=$BASE|" /etc/ai-cluster/release
OUT=$(docker exec aic-old aic update apply --yes 2>&1) && echo "$OUT" | grep -q "já está na versão mais recente" || { echo "$OUT" | tail -4; fail "a release assinada igual deveria dizer que já está na versão mais recente"; }
docker exec aic-old sed -i "s|^BASE=.*|BASE=$BASE_BAD/sig-novo|" /etc/ai-cluster/release
OUT=$(docker exec aic-old aic update apply --yes 2>&1) || { echo "$OUT" | tail -6; fail "a versão nova assinada deveria atualizar"; }
echo "$OUT" | grep -q "Atualizado para 0.2.1" || { echo "$OUT" | tail -4; fail "a atualização deveria terminar com 'Atualizado para 0.2.1'"; }
docker exec aic-old grep -qx 'VERSION=0.2.1' /etc/ai-cluster/release || fail "a release gravada deveria ser a 0.2.1"
docker exec aic-old sed -i "s|^BASE=.*|BASE=$BASE|" /etc/ai-cluster/release
falha_com "é mais antiga" docker exec aic-old aic update apply --yes
echo "OK C4b) assinada e nova atualiza; a mesma não mexe; a mais antiga pede --force"

echo "=== D) combinações recusadas ==="
# Capturar a saída antes do grep: `grep -q` fecha o pipe cedo e o pipefail contaria como falha.
refused() { out=$(inst aic-ctl "$@"); case "$out" in *"$MSG"*) ;; *) fail "esperava \"$MSG\" para: $*" ;; esac; }
MSG="só existe para x86_64" refused --role server --arch armv7
MSG="ARM de 64 bits ainda não" refused --role worker --arch aarch64 --server 1.2.3.4
MSG="este computador é x86_64" refused --role worker --arch armv7 --server 1.2.3.4
MSG="idioma inválido" refused --lang fr
MSG="não pode ser um diretório do sistema" refused --role server --arch x86_64 --models-dir /etc/modelos
MSG="--version aceita só letras" refused --role controller --arch x86_64 --version 1/2
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
espera_ate 20 docker exec aic-ctl grep -q -- "--code" /tmp/invite-en.log || fail "o convite em inglês deveria mostrar o código"
ECODE=$(docker exec aic-ctl sh -c "grep -o -- '--code [A-Za-z0-9]*' /tmp/invite-en.log | head -n 1 | cut -d' ' -f2")
docker exec aic-ctl curl -fsS "http://$CIP:8098/join-$ECODE" -o /tmp/join-en.sh || fail "o /join-<código> deveria existir"
docker exec aic-ctl sh -c "curl -fsS http://$CIP:8098/wait -o /tmp/wait-en.sh && sh -n /tmp/join-en.sh && sh -n /tmp/wait-en.sh && grep -q 'joins this cluster' /tmp/join-en.sh && grep -q 'dedicated mode' /tmp/wait-en.sh" \
  || fail "o convite em inglês deveria gerar join e wait em inglês"
echo "OK E3) convite em inglês gera join e wait em inglês"

echo "=== G) controller e gateway com systemd de mentira: habilitados no boot e isolados ==="
fake_systemd aic-sdc
OUT=$(inst aic-sdc --role server --arch x86_64) || { echo "$OUT" | tail -8; fail "install.sh server com systemd"; }
docker exec aic-sdc grep -qx 'enable ai-cluster-controller' /var/log/fake-systemctl.log || fail "o controller deveria ser habilitado no boot"
docker exec aic-sdc grep -qx 'enable ai-cluster-gateway' /var/log/fake-systemctl.log || fail "o gateway deveria ser habilitado no boot"
docker exec aic-sdc sh -c 'for s in controller gateway; do f=/etc/systemd/system/ai-cluster-$s.service; grep -qx "Restart=always" $f && grep -qx "WantedBy=multi-user.target" $f || exit 1; done' \
  || fail "as units do controller e do gateway deveriam ter Restart=always e WantedBy=multi-user.target"
docker exec aic-sdc grep -qx 'InaccessiblePaths=/etc/ai-cluster/controller.env /etc/ai-cluster/cli.env' /etc/systemd/system/ai-cluster-gateway.service \
  || fail "a unit do gateway deveria esconder controller.env e cli.env (token de operador)"
docker exec aic-sdc grep -qx 'InaccessiblePaths=/etc/ai-cluster/gateway.env' /etc/systemd/system/ai-cluster-controller.service \
  || fail "a unit do controller deveria esconder gateway.env"
docker exec aic-sdc grep -qx 'EnvironmentFile=/etc/ai-cluster/gateway.env' /etc/systemd/system/ai-cluster-gateway.service || fail "o EnvironmentFile do gateway deveria continuar"
ST=$(docker exec aic-sdc aic controller status 2>&1)
echo "$ST" | grep -q "ai-cluster-controller: ativo" || { echo "$ST"; fail "aic controller status deveria mostrar o serviço ativo"; }
echo "OK G) controller e gateway habilitados no boot, com Restart=always e InaccessiblePaths"

echo "=== G2) rollback do update com systemd de mentira ==="
# (a) o restart falha uma vez: os binários, a config e a release voltam, e o serviço antigo sobe.
docker exec aic-sdc sh -c "sed -i '/^AI_CLUSTER_ENROLL_ADDR=/d' /etc/ai-cluster/controller.env"
BIN0=$(docker exec aic-sdc sha256sum /opt/ai-cluster/bin/ai-cluster-controller | cut -d' ' -f1)
ENV0=$(docker exec aic-sdc sha256sum /etc/ai-cluster/controller.env | cut -d' ' -f1)
REL0=$(docker exec aic-sdc sha256sum /etc/ai-cluster/release | cut -d' ' -f1)
docker exec aic-sdc touch /run/fake-fail-restart
OUT=$(docker exec aic-sdc aic update apply --force --yes 2>&1); RC=$?
[ "$RC" != 0 ] || { echo "$OUT" | tail -5; fail "um restart que falha deveria sair com erro"; }
echo "$OUT" | grep -q "voltou a rodar" || { echo "$OUT" | tail -5; fail "o update desfeito deveria dizer que a versão anterior voltou a rodar"; }
[ "$(docker exec aic-sdc sha256sum /opt/ai-cluster/bin/ai-cluster-controller | cut -d' ' -f1)" = "$BIN0" ] || fail "o binário deveria ter voltado ao anterior"
[ "$(docker exec aic-sdc sha256sum /etc/ai-cluster/controller.env | cut -d' ' -f1)" = "$ENV0" ] || fail "a config deveria ter voltado (sem a chave que o update acrescentou)"
[ "$(docker exec aic-sdc sha256sum /etc/ai-cluster/release | cut -d' ' -f1)" = "$REL0" ] || fail "a release gravada deveria ter ficado como estava"
docker exec aic-sdc sh -c 'kill -0 $(cat /run/fake-units/ai-cluster-controller.pid)' || fail "o controller antigo deveria estar rodando depois do rollback"
docker exec aic-sdc test ! -e /opt/ai-cluster/bin/.prev.novo || fail "o rollback não deixa .prev.novo para trás"
echo "OK G2a) restart que falha: binário, config e release voltam; o antigo sobe"
# (b) o binário novo sai na hora: a release de mentira (o controller dá exit 1) é desfeita.
docker exec aic-sdc sed -i "s|^BASE=.*|BASE=$BASE_BAD|" /etc/ai-cluster/release
OUT=$(docker exec aic-sdc aic update apply --yes 2>&1); RC=$?
[ "$RC" != 0 ] || { echo "$OUT" | tail -5; fail "um binário que sai na hora deveria fazer o update sair com erro"; }
echo "$OUT" | grep -q "voltou a rodar" || { echo "$OUT" | tail -5; fail "o update com binário ruim deveria voltar ao antigo e dizer isso"; }
[ "$(docker exec aic-sdc sha256sum /opt/ai-cluster/bin/ai-cluster-controller | cut -d' ' -f1)" = "$BIN0" ] || fail "com binário ruim, o binário antigo deveria voltar"
espera_ate 20 docker exec aic-sdc sh -c 'kill -0 $(cat /run/fake-units/ai-cluster-controller.pid)' || fail "o controller antigo deveria estar de pé depois do rollback"
docker exec aic-sdc sh -c "sed -i 's|^BASE=.*|BASE=$BASE|' /etc/ai-cluster/release"
echo "OK G2b) binário que sai na hora: o update desfaz, volta ao antigo e sai com erro"

echo "=== H) aic update no worker Linux (--force): pid novo, registro novo, anteriores guardados ==="
docker exec aic-wk aic update status | grep -q "já está na versão mais recente" || fail "aic update status do worker deveria comparar com a origem"
WPID0=$(pid_de aic-wk /opt/ai-cluster/state/worker.pid)
REG0=$(docker exec aic-wk sh -c 'grep -c "registrado node_id=.* aceito=true" /opt/ai-cluster/state/worker.log; true')
OUT=$(docker exec aic-wk aic update apply --force --yes 2>&1) || { echo "$OUT" | tail -8; fail "aic update apply do worker"; }
echo "$OUT" | grep -q "Atualizado para" || { echo "$OUT" | tail -8; fail "o update do worker deveria terminar com 'Atualizado para'"; }
docker exec aic-wk test -x /opt/ai-cluster/bin/.prev/ai-cluster-worker || fail "o update do worker deveria guardar o binário anterior"
docker exec aic-wk test -x /opt/ai-cluster/bin/.prev/aic || fail "o update do worker deveria guardar o aic anterior"
WPID1=$(pid_de aic-wk /opt/ai-cluster/state/worker.pid)
[ -n "$WPID1" ] && [ "$WPID1" != "$WPID0" ] || fail "o worker deveria ter pid novo depois do update (antes=$WPID0 depois=$WPID1)"
docker exec aic-wk kill -0 "$WPID1" || fail "o worker novo deveria estar rodando"
espera_ate 60 docker exec aic-wk sh -c "[ \$(grep -c 'registrado node_id=.* aceito=true' /opt/ai-cluster/state/worker.log) -gt $REG0 ]" \
  || fail "o worker deveria registrar uma linha nova depois do update"
echo "OK H) update do worker: pid novo, registro novo e anteriores guardados"

echo "=== H1) rollback do worker: o worker novo sai na hora; volta o antigo, que roda e registra ==="
# A release ruim traz um ai-cluster-worker que sai com erro; o aic-wk passa a apontar para ela.
docker exec aic-wk sed -i "s|^BASE=.*|BASE=$BASE_BAD|" /etc/ai-cluster/release || fail "não apontei o worker para a release ruim"
BIN_H1=$(docker exec aic-wk sha256sum /opt/ai-cluster/bin/ai-cluster-worker | cut -d' ' -f1)
PREV_H1=$(docker exec aic-wk sha256sum /opt/ai-cluster/bin/.prev/ai-cluster-worker | cut -d' ' -f1)
RELV_H1=$(docker exec aic-wk sed -n 's/^\(VERSION\|SUMS\)=//p' /etc/ai-cluster/release)
ENV_H1=$(docker exec aic-wk sha256sum /opt/ai-cluster/worker.env | cut -d' ' -f1)
WPID_H1=$(pid_de aic-wk /opt/ai-cluster/state/worker.pid)
REG_H1=$(docker exec aic-wk sh -c 'grep -c "registrado node_id=.* aceito=true" /opt/ai-cluster/state/worker.log; true')
OUT=$(docker exec aic-wk aic update apply --yes 2>&1); RC=$?
[ "$RC" != 0 ] || { echo "$OUT" | tail -8; fail "um worker que sai na hora deveria fazer o update sair com erro"; }
echo "$OUT" | grep -q "voltou a rodar" || { echo "$OUT" | tail -8; fail "o update do worker desfeito deveria dizer que a versão anterior voltou"; }
[ "$(docker exec aic-wk sha256sum /opt/ai-cluster/bin/ai-cluster-worker | cut -d' ' -f1)" = "$BIN_H1" ] || fail "o binário do worker deveria ser o de antes do update"
[ "$(docker exec aic-wk sha256sum /opt/ai-cluster/bin/.prev/ai-cluster-worker | cut -d' ' -f1)" = "$PREV_H1" ] || fail "o .prev deveria continuar guardando o anterior"
docker exec aic-wk test ! -e /opt/ai-cluster/bin/.prev.novo || fail "o rollback não deixa .prev.novo para trás"
[ "$(docker exec aic-wk sed -n 's/^\(VERSION\|SUMS\)=//p' /etc/ai-cluster/release)" = "$RELV_H1" ] || fail "a release gravada deveria ter ficado como estava"
[ "$(docker exec aic-wk sha256sum /opt/ai-cluster/worker.env | cut -d' ' -f1)" = "$ENV_H1" ] || fail "o worker.env deveria ficar como estava"
WPID_H1B=$(pid_de aic-wk /opt/ai-cluster/state/worker.pid)
[ -n "$WPID_H1B" ] && [ "$WPID_H1B" != "$WPID_H1" ] || fail "o worker antigo deveria ter sido religado (pid antes=$WPID_H1 depois=$WPID_H1B)"
docker exec aic-wk sh -c "kill -0 $WPID_H1B && grep -q '^ai-cluster-wor' /proc/$WPID_H1B/comm" || fail "o pid guardado deveria ser do worker rodando"
espera_ate 60 docker exec aic-wk sh -c "[ \$(grep -c 'registrado node_id=.* aceito=true' /opt/ai-cluster/state/worker.log) -gt $REG_H1 ]" \
  || fail "o worker antigo deveria registrar de novo no controller depois do rollback"
docker exec aic-wk sed -i "s|^BASE=.*|BASE=$BASE|" /etc/ai-cluster/release || fail "não voltei o worker para a release boa"
echo "OK H1) worker que sai na hora: o update desfaz, o antigo roda e registra, .prev fica"

echo "=== H2) bootstrap sem systemd: binário que sai na hora é desfeito e o worker volta ==="
docker exec aic-wk sh -c "mkdir -p /tmp/tools && curl -fsS $BASE/tools.tar.gz -o /tmp/tools.tgz && tar -C /tmp/tools -xzf /tmp/tools.tgz" || fail "não consegui o pacote tools"
falha_com "--controller pede um valor" docker exec aic-wk sh /tmp/tools/bootstrap.sh --controller
TOKW=$(docker exec aic-wk sed -n 's/^AI_CLUSTER_JOIN_TOKEN=//p' /opt/ai-cluster/worker.env)
WNODE=$(docker exec aic-wk sed -n 's/^AI_CLUSTER_NODE_ID=//p' /opt/ai-cluster/worker.env)
docker exec aic-wk sh -c "printf '#!/bin/sh
exit 1
' > /tmp/bad-worker && chmod 755 /tmp/bad-worker" || fail "não consegui o binário ruim"
BINSUM0=$(docker exec aic-wk sha256sum /opt/ai-cluster/bin/ai-cluster-worker | cut -d' ' -f1)
OUT=$(docker exec aic-wk sh /tmp/tools/bootstrap.sh --controller "http://$SIP:50051" --insecure --token "$TOKW" --node-id "$WNODE" --binary /tmp/bad-worker --health-timeout 10 2>&1); RC=$?
[ "$RC" = 3 ] || { echo "$OUT" | tail -5; fail "bootstrap com binário ruim deveria sair com 3 (saiu $RC)"; }
echo "$OUT" | grep -q "rollback feito: a versão anterior está de volta" || { echo "$OUT" | tail -5; fail "o bootstrap deveria dizer que a versão anterior voltou"; }
[ "$(docker exec aic-wk sha256sum /opt/ai-cluster/bin/ai-cluster-worker | cut -d' ' -f1)" = "$BINSUM0" ] || fail "o binário anterior deveria estar em uso de novo"
docker exec aic-wk sh -c 'kill -0 $(cat /opt/ai-cluster/state/worker.pid)' || fail "o worker deveria estar rodando depois do rollback"
docker exec aic-wk test ! -e /opt/ai-cluster/worker.env.prev || fail "o rollback não deixa worker.env.prev para trás"
echo "OK H2: binário ruim desfeito pela saúde real (sem systemd), e --controller sem valor tem mensagem"

echo "=== H3) worker Linux: o pedido assinado atualiza pelo gatilho; pedido adulterado ou de outra chave não aplica ==="
# O H2 regravou o worker.env sem o gatilho: o linux-trigger (o que o join e o update fazem) o devolve. Sem systemd
# ele avisa que a atualização do controller fica manual.
docker exec aic-wk /opt/ai-cluster/libexec/setup update linux-trigger > /tmp/trigger.log 2>&1 || { cat /tmp/trigger.log; fail "o linux-trigger deveria rodar sem erro"; }
grep -q "sem systemd" /tmp/trigger.log || fail "sem systemd o gatilho deveria avisar"
# Sem systemd o worker não se declara atualizável: o rollout não o escolhe como canário (ele nunca reportaria).
docker exec aic-wk sh -c '! grep -q "^AI_CLUSTER_UPDATE_CMD=" /opt/ai-cluster/worker.env' || fail "sem systemd o worker não pode se declarar atualizável"
docker exec aic-wk test -x /opt/ai-cluster/bin/aic-update-linux && docker exec aic-wk test -s /etc/ai-cluster/release.pub || fail "o worker deveria ter o comando de atualização e a chave pinada"
VER=$(cat "$DISTU/VERSION"); SIG=$(cat "$DISTU/SHA256SUMS.sig")
UPD="runuser -u ai-cluster -- /opt/ai-cluster/bin/aic-update-linux"   # o worker roda sem root, como o serviço
# Confere sem gravar nada: assinatura boa passa; de outra chave, adulterada ou de outra versão não passam.
docker exec aic-wk sh -c "$UPD --verify $VER $BASE x x $SIG" >/dev/null 2>&1 || fail "--verify com a assinatura boa deveria passar"
falha_com "não confere com a chave" docker exec aic-wk sh -c "$UPD --verify $VER $BASE_BAD/sig-chave x x $(cat "$BADDIST/sig-chave/SHA256SUMS.sig")"
falha_com "não confere com a chave" docker exec aic-wk sh -c "$UPD --verify $VER $BASE x x $(echo "$SIG" | sed 's/^./Z/')"
falha_com "não a 9.9.9" docker exec aic-wk sh -c "$UPD --verify 9.9.9 $BASE x x $SIG"
falha_com "assinatura inválida" docker exec aic-wk sh -c "$UPD --verify $VER $BASE x x curta"
docker exec aic-wk test ! -e /opt/ai-cluster/state/update-request || fail "--verify não pode gravar pedido"
# Pedido com a assinatura de outra chave não é gravado; o da versão nova assinada é, e o ai-cluster-update (root) aplica.
falha_com "não confere com a chave" docker exec aic-wk sh -c "$UPD 0.2.1 $BASE_BAD/sig-chave x x $(cat "$BADDIST/sig-chave/SHA256SUMS.sig")"
docker exec aic-wk test ! -e /opt/ai-cluster/state/update-request || fail "pedido recusado não pode ficar gravado"
PID_H3=$(pid_de aic-wk /opt/ai-cluster/state/worker.pid)
BIN_H3=$(docker exec aic-wk sha256sum /opt/ai-cluster/bin/ai-cluster-worker | cut -d' ' -f1)
# Pedido adulterado gravado à mão (o pedido de outro usuário, p. ex.): o ai-cluster-update confere de novo e recusa.
docker exec aic-wk sh -c "printf '%s %s %s\n' 0.2.1 $BASE_BAD/sig-chave $(cat "$BADDIST/sig-chave/SHA256SUMS.sig") > /opt/ai-cluster/state/update-request && chown ai-cluster /opt/ai-cluster/state/update-request" || fail "não consegui gravar o pedido falso"
docker exec aic-wk /opt/ai-cluster/libexec/setup update linux-apply >/dev/null 2>&1 && fail "um pedido com assinatura de outra chave não pode ser aplicado"
docker exec aic-wk grep -q "não confere com a chave" /opt/ai-cluster/state/update-result || fail "o motivo da recusa deveria ficar em update-result (o worker o mostra)"
[ "$(docker exec aic-wk sha256sum /opt/ai-cluster/bin/ai-cluster-worker | cut -d' ' -f1)" = "$BIN_H3" ] && [ "$(pid_de aic-wk /opt/ai-cluster/state/worker.pid)" = "$PID_H3" ] \
  || fail "a recusa não pode trocar o binário nem reiniciar o worker"
docker exec aic-wk test ! -e /opt/ai-cluster/state/update-request || fail "o pedido recusado deveria ser apagado"
echo "OK H3a) --verify confere sem gravar; assinatura de outra chave, adulterada ou de outra versão recusa; pedido recusado não aplica e deixa o motivo"
# Ataques ao state (dono ai-cluster) que o root não pode seguir: link no pedido e link no resultado. Os alvos
# (arquivos do root, fora do state) ficam como estavam; o resultado vira arquivo comum.
docker exec aic-wk sh -c 'echo intacto > /tmp/alvo-req; echo intacto > /tmp/alvo-res' || fail "não preparei os alvos dos links"
docker exec aic-wk runuser -u ai-cluster -- ln -s /tmp/alvo-req /opt/ai-cluster/state/update-request || fail "não consegui plantar o link de pedido"
docker exec aic-wk /opt/ai-cluster/libexec/setup update linux-apply >/dev/null 2>&1 && fail "pedido com link não pode ser aplicado"
docker exec aic-wk grep -q "link simbólico" /opt/ai-cluster/state/update-result || fail "o motivo do link deveria ir para update-result"
docker exec aic-wk sh -c 'test "$(cat /tmp/alvo-req)" = intacto && test ! -L /opt/ai-cluster/state/update-request' || fail "o root não pode seguir o link do pedido"
docker exec aic-wk rm -f /opt/ai-cluster/state/update-result && docker exec aic-wk runuser -u ai-cluster -- ln -s /tmp/alvo-res /opt/ai-cluster/state/update-result || fail "não consegui plantar o link de resultado"
docker exec aic-wk sh -c "printf '0.2.1 %s %s\n' $BASE_BAD/sig-chave $(cat "$BADDIST/sig-chave/SHA256SUMS.sig") > /opt/ai-cluster/state/update-request" || fail "não gravei o pedido de teste"
docker exec aic-wk /opt/ai-cluster/libexec/setup update linux-apply >/dev/null 2>&1 && fail "pedido de outra chave não pode ser aplicado"
docker exec aic-wk sh -c 'test "$(cat /tmp/alvo-res)" = intacto && test ! -L /opt/ai-cluster/state/update-result && grep -q "não confere com a chave" /opt/ai-cluster/state/update-result' \
  || fail "o resultado não pode ir para o alvo de um link: deveria ser um arquivo comum com o motivo"
# Origem: um pedido com a assinatura boa mas de outro host não é aplicado (o BASE desta instalação ainda é o do espelho bom).
docker exec aic-wk sh -c "printf '0.2.1 %s %s\n' $BASE_BAD/sig-novo $(cat "$BADDIST/sig-novo/SHA256SUMS.sig") > /opt/ai-cluster/state/update-request" || fail "não gravei o pedido de outra origem"
docker exec aic-wk /opt/ai-cluster/libexec/setup update linux-apply >/dev/null 2>&1 && fail "pedido de outra origem não pode ser aplicado"
docker exec aic-wk grep -q "não é da origem" /opt/ai-cluster/state/update-result || fail "a recusa de origem deveria dizer o motivo"
[ "$(pid_de aic-wk /opt/ai-cluster/state/worker.pid)" = "$PID_H3" ] || fail "a recusa de origem não pode reiniciar o worker"
# A origem desta instalação passa a ser o espelho ruim (mesmo host do pedido); o pedido válido então aplica.
docker exec aic-wk sed -i "s|^BASE=.*|BASE=$BASE_BAD|" /etc/ai-cluster/release || fail "não apontei a origem do worker"
echo "OK H3c) link simbólico no pedido ou no resultado não é seguido pelo root; pedido de outra origem é recusado com motivo"

# Pedido da versão nova assinada: o worker pede (runuser, como o serviço) e o ai-cluster-update aplica: pid novo,
# release gravada e o binário anterior guardado.
docker exec aic-wk sh -c "$UPD 0.2.1 $BASE_BAD/sig-novo $(sha256sum "$BADDIST/sig-novo/SHA256SUMS" | cut -d' ' -f1) x $(cat "$BADDIST/sig-novo/SHA256SUMS.sig")" || fail "o pedido assinado deveria ser aceito"
docker exec aic-wk test -s /opt/ai-cluster/state/update-request || fail "o pedido assinado deveria ficar gravado"
docker exec aic-wk /opt/ai-cluster/libexec/setup update linux-apply > /tmp/apply-h3.log 2>&1 || { tail -8 /tmp/apply-h3.log; fail "o ai-cluster-update deveria aplicar o pedido assinado"; }
docker exec aic-wk grep -qx 'VERSION=0.2.1' /etc/ai-cluster/release || fail "a release gravada do worker deveria ser a 0.2.1"
docker exec aic-wk test -x /opt/ai-cluster/bin/.prev/ai-cluster-worker || fail "a atualização do worker deveria guardar o binário anterior"
[ "$(pid_de aic-wk /opt/ai-cluster/state/worker.pid)" != "$PID_H3" ] || fail "o worker deveria ter pid novo depois da atualização pedida"
docker exec aic-wk test ! -e /opt/ai-cluster/state/update-request || fail "o pedido aplicado deveria ser apagado"
docker exec aic-wk sed -i "s|^BASE=.*|BASE=$BASE|" /etc/ai-cluster/release || fail "não voltei a origem do worker"
echo "OK H3b) pedido assinado: o worker pede, o ai-cluster-update aplica, o pid muda e a release fica 0.2.1"

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
