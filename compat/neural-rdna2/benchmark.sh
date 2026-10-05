#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Explicit GPU execution. Compare fixed 1080p inputs in ABBA order.
set -euo pipefail
if [[ $# != 5 ]]; then
  echo "Usage: bash benchmark.sh BASELINE_BUILD OPTIMIZED_BUILD ROCM_PREFIX WEIGHTS EMPTY_OUTPUT_DIR" >&2
  exit 2
fi
baseline=$(realpath "$1")
optimized=$(realpath "$2")
rocm=$(realpath "$3")
weights=$(realpath "$4")
mkdir -p -- "$5"
out=$(realpath "$5")
[[ -z $(ls -A "$out") ]] || { echo "Use an empty benchmark output directory." >&2; exit 1; }
for name in $(compgen -v DLSS5_); do unset "$name"; done
export LD_LIBRARY_PATH="$rocm/lib/rocm_sysdeps/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export DLSS5_MH_PROD=1
run=0
for variant in baseline optimized optimized baseline; do
  ((run+=1))
  engine=$baseline
  export DLSS5_C32_SPLIT=0
  if [[ $variant == optimized ]]; then
    engine=$optimized
    export DLSS5_C32_SPLIT=1
  fi
  timeout 45s "$engine/bench-normal" --weights "$weights" --seed 7 --runs 8 \
    --output "$out/$run-$variant.rgb" > "$out/$run-$variant.log" 2>&1
  # The executable checks finite output and repeatability. Also compare every
  # final RGB byte across implementations, not just a checksum or average.
  [[ $run == 1 ]] || cmp "$out/1-baseline.rgb" "$out/$run-$variant.rgb"
  printf '%s %s PASS\n' "$run" "$variant"
done
sha256sum "$baseline/libdlss5_hip.so" "$optimized/libdlss5_hip.so" \
  "$out"/*.rgb "$out"/*.log > "$out/checksums.txt"
# Keep individual warm samples; run=0 is initialization/warm-up and excluded.
awk '/^network71/ && $2 != "run=0" { sub("gpu_ms=", "", $5); print FILENAME "\t" $5 }' \
  "$out"/*.log > "$out/gpu-samples.tsv"
printf 'ABBA comparison passed; warm GPU samples: %s/gpu-samples.tsv\n' "$out"
