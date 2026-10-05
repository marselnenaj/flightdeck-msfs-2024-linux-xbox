#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Build the pinned native renderer for gfx1030; no installation or GPU execution.
set -euo pipefail
if [[ $# != 3 ]]; then
  echo "Usage: bash build.sh SOURCE_ARCHIVE ROCM_PREFIX EMPTY_OUTPUT_DIRECTORY" >&2
  exit 2
fi
archive=$(realpath "$1")
rocm=$(realpath "$2")
adapter=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
expected=e74022396b86ccd776ac8e7aa666a91e3cc74136d428a3eb470f1ac9a05d7ed4
[[ $(sha256sum "$archive" | cut -d' ' -f1) == "$expected" ]] || {
  echo "Source archive does not match guentra/dlss5-amd-hip-linux 3c7740e6." >&2; exit 1;
}
[[ -x "$rocm/bin/hipcc" ]] || { echo "ROCm hipcc is missing." >&2; exit 1; }
mkdir -p -- "$3"
out=$(realpath "$3")
[[ -z $(ls -A "$out") ]] || { echo "Use an empty output directory." >&2; exit 1; }
mkdir "$out/source" "$out/tests"
tar -xzf "$archive" -C "$out/source" --strip-components=1 --wildcards \
  '*/hip/*' '*/src/native_frame_input_check.h' '*/LICENSE'
patch --batch --fuzz=0 -d "$out/source" -p1 < "$adapter/upstream.patch"
cp "$adapter/wave.hpp" "$out/source/hip/include/wave.hpp"
cp "$adapter/test_wave.hip" "$out/source/hip/tests/test_wave.hip"
cp "$adapter/test_c32_chain.hip" "$out/source/hip/tests/test_c32_chain.hip"
export ROCM_PATH="$rocm"
export SOURCE_DATE_EPOCH=0
# Optional private dependencies are used only by this build process.
if [[ -d "$rocm/lib/rocm_sysdeps/lib" ]]; then
  export LD_LIBRARY_PATH="$rocm/lib/rocm_sysdeps/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi
cd "$out/source/hip"
mkdir -p build
flags=(-O2 -std=c++17 --offload-arch=gfx1030 -DFLIGHTDECK_GFX1030
       -Iinclude -include include/wave.hpp)
objects=()
for name in capi frame kernels kernels_prod kernels_mh1 kernels_mh2 network gpu_interop; do
  extra=()
  [[ $name != kernels_prod ]] || extra=(-DHIP_PREPACKED_WEIGHTS)
  # Clang's default HIP compilation-unit ID hashes absolute build paths. Pin a
  # distinct ID per translation unit so identical inputs produce the same fatbin.
  "$rocm/bin/hipcc" "${flags[@]}" "${extra[@]}" "-cuid=flightdeck_gfx1030_$name" \
    -fPIC -c "src/$name.hip" -o "build/$name.o"
  objects+=("build/$name.o")
  printf 'Built %s for gfx1030\n' "$name"
done
cc -O2 -fPIC -c src/hip_load.c -o build/hip_load.o
c++ -shared -fPIC "${objects[@]}" build/hip_load.o -ldl -Wl,--allow-shlib-undefined -o "$out/libdlss5_hip.so"
# Retain the upstream PE bridge/add-on from the validated release bundle.
kernel_objects=(build/kernels.o build/kernels_prod.o build/kernels_mh1.o build/kernels_mh2.o)
for test in test_wave test_wmma test_quantize test_stages test_attention test_residual \
            test_linear test_logical_ops test_ffn_f32 test_graph_ops test_graph_blocks \
            test_graph_vit test_c32_chain test_network; do
  oracle=(-ffp-contract=off)
  # The wave test must exercise the same compiler flags as the renderer. Its
  # independent CPU oracle uses explicit sequential std::fma calls.
  [[ $test != test_wave ]] || oracle=()
  "$rocm/bin/hipcc" "${flags[@]}" "-cuid=flightdeck_gfx1030_$test" \
    "${oracle[@]}" -c "tests/$test.hip" -o "build/$test.o"
  extra=()
  [[ $test != test_network ]] || extra=(build/network.o)
  "$rocm/bin/hipcc" "build/$test.o" "${kernel_objects[@]}" "${extra[@]}" \
    -L"$rocm/lib" -Wl,-rpath,"$rocm/lib" -o "$out/tests/$test"
done
"$rocm/bin/hipcc" "${flags[@]}" -cuid=flightdeck_gfx1030_bench -c src/bench.hip -o build/bench.o
"$rocm/bin/hipcc" build/bench.o build/network.o "${kernel_objects[@]}" \
  -L"$rocm/lib" -Wl,-rpath,"$rocm/lib" -o "$out/bench-normal"
cp "$out/source/LICENSE" "$out/UPSTREAM-LICENSE"
"$rocm/bin/hipcc" --version > "$out/compiler.txt"
sha256sum "$out/libdlss5_hip.so" "$adapter/wave.hpp" "$adapter/upstream.patch" > "$out/build.sha256"
printf 'RDNA2 build complete: %s\nNo GPU tests or game sessions were run.\n' "$out"
