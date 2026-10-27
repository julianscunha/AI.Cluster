	if [ -n "$AIC_PARTUUID" ]; then
		_n=0
		while [ $_n -lt 100 ]; do
			_d=$(blkid -t PARTUUID="$AIC_PARTUUID" -o device 2>/dev/null | head -n 1)
			[ -n "$_d" ] && break
			sleep 0.2; _n=$((_n + 1))
		done
		[ -n "$_d" ] && KOPT_root=$_d
	fi
