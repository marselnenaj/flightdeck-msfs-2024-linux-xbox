# NVIDIA: Steam evidence and completion criteria

Research checked on 4 October 2026. This records the implementation decision
for maintainers; it is not a request for users to collect more diagnostics.

## What the Steam reports establish

Valve lists MSFS 2024 as playable in
[Proton 10.0-1](https://github.com/ValveSoftware/Proton/wiki/Changelog).
The relevant NVIDIA reports distinguish ordinary rendering from VR and
additional render windows:

| Report | Working case | Remaining failure / workaround |
| --- | --- | --- |
| [RTX 4080, 595.58.03, Experimental Bleeding Edge, April 2026](https://github.com/ValveSoftware/Proton/issues/8255#issuecomment-4187510640) | Normal monitor rendering | VR works with `PROTON_DISABLE_NVAPI=1 PROTON_HIDE_NVIDIA_GPU=1`; DLSS is then unavailable. |
| [RTX 5090, Mint 22, 580.126.09, GE-Proton 10-20 with VKD3D 3.0](https://github.com/ValveSoftware/Proton/issues/8255#issuecomment-4189054545) | Extended flights without pop-outs | An instrument pop-out renders briefly before the process fails. |
| [MSFS 2024 SU5, Proton 11.0.1-b2, April 2026](https://github.com/ValveSoftware/Proton/issues/8255#issuecomment-4352410256) | Multiple cameras and instrument pop-outs | Reporter confirms success with NVAPI disabled and NVIDIA hidden; their command also includes `DXVK_ASYNC=0`. This report does not isolate the effect of each variable. |

These are useful reference cases, not proof that every Steam/NVIDIA combination
works. They also do not identify the cause of Flightdeck's black primary scene.
In the [preceding multi-window investigation](https://github.com/ValveSoftware/Proton/issues/8255#issuecomment-4352255275),
the reporter observed correct rendering after the same “never been rendered to”
warning. Treating that warning as the root cause would therefore be unjustified.

The [Flightdeck RTX 4080 report](https://github.com/marselnenaj/flightdeck-msfs-2024-linux-xbox/issues/1)
reproduces a black primary viewport with driver 615.71.09, both graphics modes,
TAA/FSR2, VRR on/off and different display scaling. Its repeated swapchain
recreation is a useful symptom. GPU load alone does not establish that a correct
3D image was produced, and recreation alone does not identify its trigger.
`GDK_BACKEND=x11` selects the launcher's GTK backend; it does not establish that
the Wine process ran in a native Xorg desktop session.

## What the Flightdeck source comparison establishes

The runner is Xodus Proton `7c0b435495814349735c913fde78da906aecea52`,
with Wine `b1dd32734a34472a28eb5be9922df06e07ac0834`. Its original rendering
libraries identify themselves as DXVK `v3.0.2-10-g6227b633e8d5289` and VKD3D
`651f17762e439feeef22dbb4ee7eff167ee503d4` (3.1.0). This is not the same frozen
stack as either successful April report.

| Concern | Checked implementation | Consequence |
| --- | --- | --- |
| Proton launch options | `flightdeck/graphics.py` translates NVAPI disabling and NVIDIA hiding into Wine/DXVK settings and DLL overrides. | Passing additional `PROTON_*` strings to direct Wine is not an implementation of a Proton feature. The reported Steam NVAPI/hiding workaround is already represented. |
| MSFS intro argument | The successful April RTX 4080 VR command also contains `-FastLaunch`; the earlier comparison omitted it. Matching Windows Store black-main-view reports independently describe success with this argument. | The Python and Rust bridges in the 0.2.0-dev.1 source pass it to the actual game process. See the [source reports and limits](nvidia-renderer.md#intro-startup-workaround). |
| NVIDIA files | NVAPI comes from the selected runner; NGX comes from the installed host driver. | Keep driver and runner libraries matched instead of downloading arbitrary NVIDIA DLLs. |
| Physical adapter | DXGI is selected by a Vulkan device UUID, retained when the vendor name is hidden. D3D12 receives the DXGI adapter. | Do not reintroduce the host-name filter that broke hidden-vendor mode. |
| Environment propagation | Backend → play script → launch script → Xodus → Wine bridge preserves these settings. | No evidence that the relevant NVIDIA options disappear before the game starts. |
| DXGI/D3D12 installation | Prefix preparation copies DXGI/D3D11 and the D3D12 DLL pair from the runner. | A Wine executable path alone is not a complete renderer selection. |
| Low-latency Vulkan support | VKD3D enables `VK_NV_low_latency2` independently of NVAPI. | The previous compatibility mode could still enter the driver-specific swapchain path. 0.1.11 also corrects DXVK’s ignored opt-out and excludes the extension in both APIs by default. NVIDIA features remain an explicit choice. |
| MSFS-specific Proton defaults | The pinned `proton` script has `noopwr` for MSFS 2020 (1250410), not an MSFS 2024 (2537590) entry. | Do not copy an unrelated 2020 workaround or assume a Steam AppID supplies a missing 2024 fix. |
| Store game loading | `scripts/runtime/xodus-wine-launch` preserves Xodus' inherited file descriptors using `pass_fds`. Proton's `run_proc` calls `subprocess.call` without them. | Replacing the bridge with `proton run` would lose the Store loader inputs. A full Proton integration must preserve that contract and prefix ownership. |

The [VKD3D extension opt-out](https://github.com/HansKristian-Work/vkd3d-proton/blob/master/README.md)
and the three upstream swapchain lifetime backports are already included in the
[0.1.9 renderer](nvidia-renderer.md). The backports fix concrete upstream
defects; the Steam reports do not prove that those defects caused this black
primary viewport. The separate DXVK source correction in 0.1.11 makes its
documented opt-out effective as well; it is not inferred from that warning.

## Route to confirmed NVIDIA compatibility

1. Keep the 0.1.11 graphics bundle frozen and reproducible: the existing
   patched D3D12 pair and matched DXVK libraries with the corrected opt-out.
   Automatic and Compatibility apply the complete profile. Preserve an
   unmodified runner comparison and existing
   game, prefix and save data.
2. Qualify it with the actual Store edition of MSFS 2024 on NVIDIA. The minimum
   coverage for the reported failures is one RTX 40-series and one RTX 50-series
   machine. For each, record the exact driver, runner, game version and mode.
   Verify the main-menu globe, free-flight map and primary cockpit view; open,
   use and close a second render window; then keep flying in the first window.
   Repeat after exiting and starting the simulator again. A working secondary
   window with a black primary window is a failure.
3. If the main scene remains black, compare a complete Proton graphics stack
   from a working NVIDIA/MSFS reference on the same machine, keeping the Store
   loader and game version fixed. Change one layer per comparison: the matched
   DXGI/VKD3D stack, then Wine if necessary. A historical success report alone
   does not qualify a version for today's game. Use the first passing/failing
   pair to locate the regression before adding another patch.
4. Describe only a combination that passes the game checks as verified NVIDIA
   support. Verify automatic NVIDIA features separately; if only the
   compatibility path passes, document that DLSS/Reflex/Frame Generation are
   unavailable in that supported configuration.

The experimental [Proton selection](runtime.md#experimental-proton-selection)
now copies an installed runner and its graphics libraries into a separate trial
profile. A portable descriptor-backed launch bridge allows unmodified Wine to
load Xodus' licensed executable and DLL images. The original Store/GDK libraries
remain installed, and account/save helpers retain the original runner. This is
not a call to Steam's `proton run`; the inherited-descriptor and prefix contract
is preserved. Synthetic loader and renderer tests do not establish full MSFS
or NVIDIA compatibility. The 0.1.18 follow-up confirms that the layout backport
alone did not resolve the reporting user's black main view.

The local D3D12 clear/readback/present, shader and multi-window checks passed on
an RX 6900 XT. There is no local NVIDIA GPU for the game qualification above.
The 0.1.9 release therefore remains **unverified for NVIDIA MSFS rendering**;
additional web research or another passing AMD probe cannot close that gap.
