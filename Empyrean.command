#!/bin/sh
# Double-click in Finder to run the local Empyrean world and log in. See local.py.
cd "$(dirname "$0")" && exec python3 local.py "$@"
