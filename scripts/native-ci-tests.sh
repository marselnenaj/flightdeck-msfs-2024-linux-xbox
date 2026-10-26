#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Ubuntu's hosted runner restricts UID-map creation even for the bubblewrap
# fixture inside our capability-free test process. Allow user namespaces only
# for this disposable CI test interval, then restore the runner's setting.
set -euo pipefail

if [[ "${GITHUB_ACTIONS:-}" != true ]]; then
    exec "$@"
fi

namespace_key=kernel.apparmor_restrict_unprivileged_userns
namespace_original=$(sysctl -n "$namespace_key" 2>/dev/null || true)
namespace_changed=false
restore_namespace_setting() {
    local test_status=$?
    trap - EXIT
    if [[ "$namespace_changed" == true ]]; then
        sudo -n sysctl -w "$namespace_key=$namespace_original" || exit 1
    fi
    exit "$test_status"
}
trap restore_namespace_setting EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
if [[ "$namespace_original" == 1 ]]; then
    sudo -n sysctl -w "$namespace_key=0"
    namespace_changed=true
fi
"$@"
