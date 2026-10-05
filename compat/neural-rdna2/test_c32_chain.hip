// SPDX-License-Identifier: MIT
// Exercise the real C32 scheduler, including its inter-block buffer format.
// Reuse the independent scalar oracle from the pinned upstream block test.
#define main upstream_block_oracle_main
#include "test_graph_blocks.hip"
#undef main

int main() {
    try {
        const std::string dir = std::string(std::getenv("DLSS5_GRAPH_TEST_BUILD")
            ? std::getenv("DLSS5_GRAPH_TEST_BUILD") : "linux/build/graph-audit") + "/c32-chain";
        std::filesystem::create_directories(dir);
        Scratch scratch;
        scratch.create();
        std::mt19937 rng(0xc32c4a1);
        std::vector<float> fw(8736), aw(8225), input(64 * 32);
        for (auto& v : fw) v = ff(float(int(rng() % 15) - 7) / 32);
        for (auto& v : aw) v = ff(float(int(rng() % 15) - 7) / 32);
        for (auto& v : input) v = hh(float(int(rng() % 21) - 10) * .111f);
        for (unsigned j = 0; j < 32; ++j) {
            fw[8704 + j] = hh(.71f + j * .003f);
            aw[8193 + j] = hh(.57f + j * .002f);
        }
        aw[8192] = 1.5f;
        std::vector<__half> half(input.size());
        for (size_t i = 0; i < input.size(); ++i) half[i] = __float2half(input[i]);
        DeviceBuf in(half.size() * 2), out(input.size() * 4);
        in.upload(half.data(), half.size() * 2);
        unsigned bad = 0;
        unsetenv("DLSS5_C32_PROD");
        // Encoder and decoder chains use the same logic but different block IDs.
        for (bool split : {false, true}) for (int first : {1, 66}) {
            setenv("DLSS5_C32_SPLIT", split ? "1" : "0", 1);
            auto expected = input;
            const __half* next = in.as<__half>();
            bool in8 = false;
            const unsigned shifts[] = {0, 3, 1, 2};
            for (unsigned j = 0; j < 4; ++j) {
                const int id = first + int(j);
                save(dir, "block" + std::to_string(id) + "-ffn", fw);
                save(dir, "block" + std::to_string(id) + "-attention", aw);
                Block block;
                block.create(dir, id, 32, 8, 8, shifts[j], true);
                const bool out8 = c32_byte_chain(id + 1);
                if (out8 != (!split && j < 3)) throw std::runtime_error("C32 buffer format policy");
                block.run(nullptr, out.as<float>(), scratch, nullptr, false, next,
                          true, true, false, true, in8, out8);
                hip_check(hipDeviceSynchronize(), "C32 chain");
                if (block.chain) for (auto& v : expected) v = ff(v);
                expected = reference(expected, fw, aw, 32, shifts[j], true);
                std::vector<float> actual(expected.size());
                out.download(actual.data(), actual.size() * 4);
                size_t mismatches = 0;
                for (size_t i = 0; i < actual.size(); ++i)
                    if (!std::isfinite(actual[i]) || actual[i] != expected[i]) {
                        if (mismatches < 2) std::printf("C32 chain id=%d i=%zu got=%g expected=%g\n", id, i, actual[i], expected[i]);
                        ++mismatches;
                    }
                bad += mismatches;
                std::printf("C32 %s id=%d shift=%u in8=%d out8=%d outputs=%zu exact_mismatches=%zu\n",
                    split ? "split" : "fused", id, shifts[j], int(in8), int(out8), actual.size(), mismatches);
                next = out8 ? reinterpret_cast<const __half*>(scratch.f8.p) : scratch.out16.as<__half>();
                in8 = out8;
            }
        }
        return bad ? 1 : 0;
    } catch (const std::exception& e) {
        std::fprintf(stderr, "C32 chain: %s\n", e.what());
        return 1;
    }
}
