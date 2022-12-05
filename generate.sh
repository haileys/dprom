#!/bin/bash
set -euo pipefail

if ! which -q zbus-xmlgen &>/dev/null; then
    echo "zbus-xmlgen missing! hint: cargo install zbus-xmlgen" >&2
    exit 1
fi

set -x
zbus-xmlgen dbus/org.hails.dprom.Counter1.xml > src/dbus/counter.rs
zbus-xmlgen dbus/org.hails.dprom.DProm1.xml > src/dbus/dprom.rs
zbus-xmlgen dbus/org.hails.dprom.Gauge1.xml > src/dbus/gauge.rs
