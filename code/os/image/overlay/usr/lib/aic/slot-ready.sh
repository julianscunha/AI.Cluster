# Critério para confirmar o slot, por papel (lido de /data/role: worker, controller ou gateway; sem o
# arquivo é worker). Carregado com `.` por aic-slot-commit; testado por tests/os-slot-ready.sh.
#   worker      registrou-se no controller (linha "registrado ... aceito=true" no log)
#   controller  o serviço respondeu na porta (50051); gateway, idem (8080). Registrar-se não vale:
#               o controller não se registra em ninguém, e confirmar sem o serviço de pé trancaria
#               o aparelho num slot que não serve. AIC_ROLE_PORT (ou /data/role.port) troca a porta.
# AIC_ROLE_FILE, AIC_WORKER_LOG e AIC_NC existem para teste.
slot_role() {
	local r
	r=$(head -n 1 "${AIC_ROLE_FILE:-/data/role}" 2>/dev/null | tr -d '[:space:]')
	case "$r" in controller | gateway) echo "$r" ;; *) echo worker ;; esac
}

slot_ready() {
	local role port
	role=$(slot_role)
	case "$role" in
	worker) grep -q "registrado node_id=.* aceito=true" "${AIC_WORKER_LOG:-/var/log/ai-cluster-worker.log}" 2>/dev/null ;;
	*)
		port=${AIC_ROLE_PORT:-$(head -n 1 "${AIC_ROLE_FILE:-/data/role}.port" 2>/dev/null | tr -dc '0-9')}
		[ -n "$port" ] || { [ "$role" = controller ] && port=50051 || port=8080; }
		${AIC_NC:-nc} -z -w 2 127.0.0.1 "$port" 2>/dev/null
		;;
	esac
}
