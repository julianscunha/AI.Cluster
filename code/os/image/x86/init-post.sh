	if [ -n "$AIC_PARTUUID" ]; then
		_n=0
		while [ $_n -lt 150 ]; do
			_d=$(blkid -t PARTUUID="$AIC_PARTUUID" -o device 2>/dev/null)
			[ -n "$_d" ] && break
			sleep 0.2; _n=$((_n + 1))
		done
		# dois discos com o mesmo PARTUUID (imagem gravada sem o install-disk.sh): não adivinha
		case "$_d" in
		*"
"*) echo "AI Cluster OS: mais de um disco com PARTUUID=$AIC_PARTUUID; abortando" >&2; _d= ;;
		esac
		[ -n "$_d" ] && KOPT_root=$_d
	fi
