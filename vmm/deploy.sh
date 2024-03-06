#!/bin/bash

set -o errexit
set -o nounset
set -o pipefail
set -o xtrace

readonly TARGET_HOST=tothalex@10.55.0.1
readonly TARGET_PATH=/home/tothalex/ruspberry
readonly SOURCE_PATH=target/debug/vmm

rsync ${SOURCE_PATH} ${TARGET_HOST}:${TARGET_PATH}
