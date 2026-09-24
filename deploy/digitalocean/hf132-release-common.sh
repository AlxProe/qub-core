#!/usr/bin/env bash
# Shared HF132 deployment operations. This file performs no action when sourced.

hf132_atomic_install() {
    local source=$1 destination=$2 mode=$3 temporary
    [[ "$destination" == /* ]] || return 1
    sudo test -f "$source" && ! sudo test -L "$source" || return 1
    ! sudo test -L "$destination" || return 1
    temporary=$(sudo mktemp "${destination}.hf132.XXXXXX") || return $?
    if ! sudo install -m"$mode" -o root -g root -- "$source" "$temporary"; then
        sudo rm -f -- "$temporary"
        return 1
    fi
    if ! sudo mv -fT -- "$temporary" "$destination"; then
        sudo rm -f -- "$temporary"
        return 1
    fi
}

hf132_stop_unit() {
    sudo timeout --signal=TERM --kill-after=10s 60s systemctl stop "$1" || return $?
    [ "$(systemctl show -p MainPID --value "$1")" = 0 ] || return 1
    ! systemctl is-active --quiet "$1"
}

hf132_unit_owns_listener() {
    local unit=$1 port=$2 pid
    systemctl is-active --quiet "$unit" || return 1
    pid=$(systemctl show -p MainPID --value "$unit") || return $?
    [[ "$pid" =~ ^[1-9][0-9]*$ ]] || return 1
    # Match the selected unit's process, not an unrelated listener on that port.
    sudo ss -H -ltnp "sport = :$port" | grep -F "pid=$pid," >/dev/null
}
