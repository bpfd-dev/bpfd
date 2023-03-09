
#!/bin/bash

# Copyright 2023 The Kubernetes Authors.
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

# This script is used to manually run golangci-lint on the bpfd-operator code.
# In bpfd's github actions a dedicated action is used to run golangci-lint. It's
# important that the version here matches the version defined in `.github/workflows/build.yml`.
set -o errexit
set -o nounset
set -o pipefail

readonly VERSION="v1.51.1"
readonly KUBE_ROOT=$(dirname "${BASH_SOURCE}")/..

cd "${KUBE_ROOT}"

# See configuration file in ${KUBE_ROOT}/.golangci.yml.
mkdir -p cache

docker run --rm -v $(pwd)/cache:/cache -v $(pwd):/app --security-opt="label=disable" -e GOLANGCI_LINT_CACHE=/cache -w /app "golangci/golangci-lint:$VERSION" golangci-lint run -v

# ex: ts=2 sw=2 et filetype=sh
