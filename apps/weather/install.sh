#!/usr/bin/env bash
# Install weather: the Nyx suite installer, for this app. Options pass through
# (see ../../install.sh).
here=$(cd "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
exec "$here/../../install.sh" weather "$@"
