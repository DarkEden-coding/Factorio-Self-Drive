#!/bin/sh
set -eu
exec python3 "$(dirname "$0")/collect-game-data.py" "$@"
