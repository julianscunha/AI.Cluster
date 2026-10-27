# AI Cluster OS: root=PARTUUID=… (a identidade do slot é a partição da GPT, não o rótulo do ext4)
case "$KOPT_root" in
PARTUUID=*) AIC_PARTUUID=${KOPT_root#PARTUUID=}; KOPT_root_wait= ;;
*) KOPT_root_wait=$KOPT_root ;;
esac
