#!/bin/sh
set -eu

# CUBE_TEMPLATE_WARMUP is supplied only while creating the template.  It is
# intentionally unset in the image so restored instances must receive their
# own runtime configuration and short-lived credentials.
exec /usr/local/bin/erp-agent-runtime
