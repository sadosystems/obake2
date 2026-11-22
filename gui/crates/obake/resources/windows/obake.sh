#!/usr/bin/env sh

if [ "$OBAKE_WSL_DEBUG_INFO" = true ]; then
	set -x
fi

OBAKE_PATH="$(dirname "$(realpath "$0")")"

IN_WSL=false
if [ -n "$WSL_DISTRO_NAME" ]; then
	# $WSL_DISTRO_NAME is available since WSL builds 18362, also for WSL2
	IN_WSL=true
fi

if [ $IN_WSL = true ]; then
    WSL_USER="$USER"
    if [ -z "$WSL_USER" ]; then
        WSL_USER="$USERNAME"
    fi
    "$OBAKE_PATH/obake.exe" --wsl "$WSL_USER@$WSL_DISTRO_NAME" "$@"
    exit $?
else
    "$OBAKE_PATH/obake.exe" "$@"
    exit $?
fi
