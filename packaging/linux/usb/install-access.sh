#!/bin/sh
set -eu
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH
[ "$(id -u)" = 0 ] || { echo "Bir defalık kurulum yönetici yetkisi gerektirir." >&2; exit 1; }
[ "$#" = 1 ] || exit 1
src=$1
for directory in /usr/libexec /usr/share/mause /usr/share/polkit-1/actions /usr/share/polkit-1/rules.d; do
    [ -d "$directory" ] || install -d -o root -g root -m 755 "$directory"
done
install -o root -g root -m 755 "$src/mause-usb-helper" /usr/libexec/mause-usb-helper
install -o root -g root -m 644 "$src/org.mause.usb.policy" /usr/share/polkit-1/actions/org.mause.usb.policy
install -o root -g root -m 644 "$src/50-mause-usb.rules" /usr/share/polkit-1/rules.d/50-mause-usb.rules
install -o root -g root -m 644 "$src/usb-access.version" /usr/share/mause/usb-access.version
