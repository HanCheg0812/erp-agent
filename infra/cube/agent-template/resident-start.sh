#!/bin/sh
set -eu

# CUBE_TEMPLATE_WARMUP is supplied only while creating the template.  It is
# intentionally unset in the image so restored instances must receive their
# own runtime configuration and short-lived credentials.
case "$(id -u)" in
  0)
    exec /usr/bin/setpriv --reuid=10001 --regid=10001 --init-groups /usr/local/bin/erp-agent-runtime
    ;;
  10001)
    exec /usr/local/bin/erp-agent-runtime
    ;;
  *)
    echo 'Unexpected Cube runtime user' >&2
    exit 1
    ;;
esac
