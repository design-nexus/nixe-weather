#!/usr/bin/env bash
# Install weather: the Nixe suite installer, for this app. Options pass through
# (see ../../install.sh).
here=$(cd "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
exec "$here/../../install.sh" weather "$@"
