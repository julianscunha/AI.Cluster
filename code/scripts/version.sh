#!/bin/sh
# Versão do produto (ADR-029): X.Y.Z do [workspace.package] de code/Cargo.toml, a mesma para crates, setup,
# binários Linux, imagens do SO (x86 e Pi), instalador e pasta dedicado. AIC_RELEASE só sobrescreve (teste).
# uso: v=$(sh code/scripts/version.sh)
v=${AIC_RELEASE:-$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\([^"]*\)".*/\1/p' "$(dirname "$0")/../Cargo.toml" | head -n 1)}
case $v in
	[0-9]*.[0-9]*.[0-9]*) ;; *) echo "versão inválida '$v' (esperado X.Y.Z)" >&2; exit 1;;
esac
case $v in *[!0-9.]*|*..*|*.) echo "versão inválida '$v' (esperado X.Y.Z)" >&2; exit 1;; esac
printf '%s\n' "$v"
