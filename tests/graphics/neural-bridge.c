// SPDX-License-Identifier: MIT
// PE -> SysV bridge. Optional inference is an explicit GPU test.
#include <windows.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// Pinned dlss5_capi.h V2/V3 ABI (upstream MIT); host buffers are packed 1080p.
typedef struct {
    unsigned struct_size, flags;
    const float *rgba;
    float *rgb;
    const float *motion;
    unsigned motion_width, motion_height;
    float motion_uv_scale_x, motion_uv_scale_y;
    unsigned seed, reset;
    float paper_white, transfer, color;
} Frame;
typedef struct {
    unsigned struct_size;
    void *in_handle, *out_handle;
    unsigned width, height, dxgi_format, seed, flags;
    float paper_white, transfer, color;
    unsigned handle_gen, temporal_gen;
} RawFrame;

int main(int argc, char **argv) {
    HMODULE dll = LoadLibraryA("dlss5_hip.dll");
    if (!dll) { fprintf(stderr, "HIP DLL load failed: %lu\n", GetLastError()); return 1; }
    int (*frame)(const void *) = (void *)GetProcAddress(dll, "dlss5_run_frame");
    const char *(*error)(void) = (void *)GetProcAddress(dll, "dlss5_last_error");
    if (!frame || !error) { fprintf(stderr, "HIP exports missing\n"); return 1; }
    int rc = frame(NULL);
    const char *message = error();
    // "not initialized" comes from the Linux library. Missing/incompatible
    // bridges have a different PE-side error and must fail this test.
    if (rc != -1 || !message || strcmp(message, "not initialized")) {
        fprintf(stderr, "Bridge call failed: %d %s\n", rc, message ? message : "(null)");
        return 1;
    }
    puts("PASS PE-to-Linux frame ABI; no model loaded, no inference claimed");
    if (argc == 1) return 0;
    const int model = argc == 4 && !strcmp(argv[1], "--model");
    if (!model && (argc != 3 || strcmp(argv[1], "--synthetic"))) return 2;
    int (*init)(const char *, int) = (void *)GetProcAddress(dll, "dlss5_init");
    int (*run)(const float *, float *, unsigned) = (void *)GetProcAddress(dll, "dlss5_run");
    int (*find)(const char *) = (void *)GetProcAddress(dll, "dlss5_find_device");
    void (*shutdown)(void) = (void *)GetProcAddress(dll, "dlss5_shutdown");
    int (*raw)(const void *) = (void *)GetProcAddress(dll, "dlss5_run_frame_raw_gpu");
    if (!init || !run || !find || !shutdown || !raw) return 2;
    int device = find("AMD Radeon RX 6900 XT");
    if (device < 0 || init(argv[2], device)) {
        fprintf(stderr, "Model init failed: %s\n", error()); return 1;
    }
    const size_t pixels = 1920u * 1080u;
    float *input = malloc(pixels * 4 * sizeof(float));
    float *output = malloc(pixels * 3 * sizeof(float));
    float *first = malloc(pixels * 3 * sizeof(float));
    if (!input || !output || !first) return 2;
    float *reference = NULL;
    if (model) {
        reference = malloc(pixels * 3 * sizeof(float));
        FILE *file = fopen(argv[3], "rb");
        if (!reference || !file) return 2;
        const size_t n = fread(reference, sizeof(float), pixels * 3, file);
        const int trailing = fgetc(file);
        fclose(file);
        if (n != pixels * 3 || trailing != EOF) return 2;
    }
    for (size_t p = 0; p < pixels; ++p) {
        input[p * 4] = .25f; input[p * 4 + 1] = .25f;
        input[p * 4 + 2] = .75f; input[p * 4 + 3] = 1.f;
        if (model) {
            input[p * 4] = (float)(p % 1920) / 1919.f;
            input[p * 4 + 1] = (float)(p / 1920) / 1079.f;
            input[p * 4 + 2] = .25f;
        }
    }
    for (unsigned repeat = 0; repeat < 2; ++repeat) {
        LARGE_INTEGER frequency, start, end;
        QueryPerformanceFrequency(&frequency); QueryPerformanceCounter(&start);
        if (run(input, output, 7)) {
            fprintf(stderr, "Neural frame failed: %s\n", error()); return 1;
        }
        QueryPerformanceCounter(&end);
        size_t bad = 0;
        for (size_t p = 0; p < pixels; ++p) {
            // Independent closed-form oracle for test_network.hip's synthetic
            // prefix -> skip -> post-head: G=.25 - .005859375/4, B unchanged.
            if (!model) bad += output[p * 3 + 1] != .24853515625f || output[p * 3 + 2] != .75f;
            for (unsigned c = 0; c < 3; ++c) {
                bad += !isfinite(output[p * 3 + c]);
                if (model) bad += memcmp(output + p * 3 + c, reference + p * 3 + c, sizeof(float)) != 0;
            }
        }
        if (repeat) bad += memcmp(first, output, pixels * 3 * sizeof(float)) != 0;
        else memcpy(first, output, pixels * 3 * sizeof(float));
        printf("Wine %s 1080p repeat=%u host_ms=%.3f mismatches=%zu\n", model ? "real-model" : "synthetic", repeat,
               1000.0 * (double)(end.QuadPart - start.QuadPart) / (double)frequency.QuadPart, bad);
        if (bad) return 1;
    }
    // The game uses the V3 packed readback/upload-buffer route. Compare its
    // entire output to the separate V2 float-frame path, with nontrivial alpha.
    unsigned char *packed = malloc(pixels * 4), *result = malloc(pixels * 4);
    if (!packed || !result) return 2;
    const unsigned char bgra[4] = {192, 64, 230, 173};
    for (size_t p = 0; p < pixels; ++p) {
        memcpy(packed + p * 4, bgra, 4);
        input[p * 4] = 230.f / 255.f; input[p * 4 + 1] = 64.f / 255.f;
        input[p * 4 + 2] = 192.f / 255.f; input[p * 4 + 3] = 173.f / 255.f;
    }
    Frame f = {0};
    f.struct_size = sizeof(f); f.rgba = input; f.rgb = output;
    f.seed = 7; f.reset = 1; f.paper_white = f.transfer = f.color = 1.f;
    RawFrame r = {0};
    r.struct_size = sizeof(r); r.in_handle = packed; r.out_handle = result;
    r.width = 1920; r.height = 1080; r.dxgi_format = 87; r.seed = 7;
    r.flags = 4; r.paper_white = r.transfer = r.color = 1.f;
    if (frame(&f) || raw(&r)) {
        fprintf(stderr, "Packed frame bridge failed: %s\n", error()); return 1;
    }
    size_t bad = 0, changed = 0;
    for (size_t p = 0; p < pixels; ++p) {
        for (unsigned c = 0; c < 3; ++c) {
            const float v = output[p * 3 + (2 - c)];
            if (!isfinite(v)) return 1;
            const unsigned char expected = (unsigned char)floorf(fminf(1.f, fmaxf(0.f, v)) * 255.f + .5f);
            bad += result[p * 4 + c] != expected;
            changed += expected != packed[p * 4 + c];
        }
        bad += result[p * 4 + 3] != 173;
    }
    printf("Wine packed V3 BGRA8 bytes=%zu mismatches=%zu changed_channels=%zu\n", pixels * 4, bad, changed);
    if (bad || !changed) return 1;
    free(result); free(packed);
    shutdown(); free(reference); free(first); free(output); free(input);
    puts(model ? "PASS Wine-to-gfx1030 real-model frames; game interception and model quality unverified"
               : "PASS Wine-to-gfx1030 synthetic frames; game interception and model quality unverified");
    return 0;
}
