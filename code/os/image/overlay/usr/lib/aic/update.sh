#!/bin/sh
# Registro da atualização, comum ao ARM e ao x86 (a camada A/B, ab.sh, é de cada plataforma).
#
# Registra a atualização que trouxe o slot confirmado, para o `aic update status` mostrar. O worker grava a
# versão que tentou em update-tried; se ela é a que roda agora, a atualização deu certo. Uma vez por versão:
# um boot comum não reescreve a data.
update_record() { # update_record <slot>
	_dir=${AIC_STATE_DIR:-/data/state}
	_tried=$(cat "$_dir/update-tried" 2>/dev/null)
	_rel=$(cat "${AIC_RELEASE_FILE:-/etc/aic-release}" 2>/dev/null)
	[ -n "$_tried" ] && [ "$_tried" = "$_rel" ] || return 0
	grep -q "versao=$_rel " "$_dir/update-last" 2>/dev/null && return 0
	printf 'ok versao=%s slot=%s em=%s\n' "$_rel" "$1" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$_dir/update-last"
}
