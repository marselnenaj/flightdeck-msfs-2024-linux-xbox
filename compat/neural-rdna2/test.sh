#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Explicit GPU execution, synthetic data only. Run on an idle gfx1030 device.
set -euo pipefail
if [[ $# != 2 ]]; then
  echo "Usage: bash test.sh BUILD_DIRECTORY ROCM_PREFIX" >&2; exit 2
fi
out=$(realpath "$1")
rocm=$(realpath "$2")
export LD_LIBRARY_PATH="$rocm/lib/rocm_sysdeps/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export DLSS5_GRAPH_TEST_BUILD="$out/tests"
unset DLSS5_C32_PROD DLSS5_C32_SPLIT DLSS5_VIT_FFN_SPLIT DLSS5_MH_PROD
for test in test_wave test_wmma test_quantize test_stages test_attention test_residual \
            test_linear test_logical_ops test_ffn_f32 test_graph_ops test_graph_blocks test_graph_vit test_c32_chain; do
  if timeout 45s "$out/tests/$test" > "$out/tests/$test.log" 2>&1; then
    printf '%s PASS\n' "$test"
  else
    cat "$out/tests/$test.log" >&2; exit 1
  fi
done
# Uses synthetic tables (~600 MB on disk), executes every block at native 1080p
# and validates trace, output, seed/history, loader errors and deterministic reuse.
# The launcher uses the same MH_PROD=1 and C32_SPLIT=1 policy for gfx1030.
if DLSS5_MH_PROD=1 DLSS5_C32_SPLIT=1 timeout 60s "$out/tests/test_network" > "$out/tests/test_network.log" 2>&1; then
  printf 'test_network PASS\n'
else
  cat "$out/tests/test_network.log" >&2; exit 1
fi
