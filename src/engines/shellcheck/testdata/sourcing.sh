#!/bin/bash
# Sources a sibling: followed only with external_sources (or when it is
# also an input), its findings reported only with check_sourced.

source "$(dirname "$0")/lib.sh"
# shellcheck source=lib.sh
. ./lib.sh
# shellcheck source-path=SCRIPTDIR
source lib.sh
lib_greet "$LIB_VALUE"
echo "$LIB_VALU"
source ./does-not-exist.sh
source
. "$1"
