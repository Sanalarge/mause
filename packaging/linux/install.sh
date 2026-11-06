#!/bin/sh
set -e
cd "$(dirname "$0")"
prefix=/usr/local

if [ "$(id -u)" -ne 0 ]; then
    echo "Root olarak çalıştırın: sudo $0 $*" >&2
    exit 1
fi

files="$prefix/bin/mause
$prefix/share/applications/mause.desktop
$prefix/share/icons/hicolor/scalable/apps/mause.svg
/etc/udev/rules.d/60-mause.rules
/etc/modules-load.d/mause.conf
/usr/libexec/mause-usb-helper
/usr/share/polkit-1/actions/org.mause.usb.policy
/usr/share/polkit-1/rules.d/50-mause-usb.rules
/usr/share/mause/usb-access.version"

if [ "$1" = "--remove" ]; then
    echo "$files" | while read -r f; do rm -f "$f"; done
    echo "Mause kaldırıldı."
    exit 0
fi

install -Dm755 mause "$prefix/bin/mause"
install -Dm644 mause.desktop "$prefix/share/applications/mause.desktop"
install -Dm644 mause.svg "$prefix/share/icons/hicolor/scalable/apps/mause.svg"
install -Dm644 60-mause.rules /etc/udev/rules.d/60-mause.rules
install -Dm644 mause-modules.conf /etc/modules-load.d/mause.conf
sh ./usb/install-access.sh "$(pwd)/usb"
sh ./postinst

if ! ldd "$prefix/bin/mause" | grep -q "not found"; then
    echo "Mause kuruldu. Uygulama menüsünden ya da 'mause' komutuyla açın."
else
    echo "Mause kuruldu ama eksik kütüphane var:" >&2
    ldd "$prefix/bin/mause" | grep "not found" >&2
    echo "PipeWire kütüphanesini kurun (ör. Arch: pacman -S libpipewire)." >&2
fi
