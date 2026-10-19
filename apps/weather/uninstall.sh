#!/usr/bin/env bash
# Remove weather (--purge also removes its settings and cached forecasts).
here=$(cd "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
exec "$here/../../uninstall.sh" weather "$@"
