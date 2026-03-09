#!/bin/sh
set -e

exec /usr/local/bin/ghost daemon "$@"
