#!/bin/sh
set -eu
umask 077
export XDG_CONFIG_HOME=/data
exec python3 /opt/bridge/ingress.py
