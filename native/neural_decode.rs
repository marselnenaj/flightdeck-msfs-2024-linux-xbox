// SPDX-License-Identifier: MIT
//! CPU-only coefficient layouts from the pinned MIT upstream converter.
//! No NVIDIA code/weights. These AMD-consumer maps remain experimental.
//! Source: guentra/dlss5-amd-hip-linux 3c7740e6, linux/dlssnr/convert_dll.py.
use crate::{Error, Result, error::require};
use std::collections::BTreeMap;

const INVALID: &str = "Invalid or unsupported NR weight archive.";
const C32: [usize; 32] = [
    0, 1, 4, 5, 8, 9, 12, 13, 2, 3, 6, 7, 10, 11, 14, 15, 16, 17, 20, 21, 24, 25, 28, 29, 18, 19,
    22, 23, 26, 27, 30, 31,
];

pub fn record_sizes() -> BTreeMap<String, usize> {
    let mut sizes = BTreeMap::new();
    for b in 0..71 {
        let layers = if (23..31).contains(&b) || (40..48).contains(&b) {
            let mut v = vec![524288, 263168, 917568, 263168];
            if b == 30 {
                v.push(524304);
            }
            v
        } else if (31..39).contains(&b) {
            vec![4194320, 4196352, 3145856, 2, 1050624]
        } else {
            vec![match b {
                0 => 21696,
                4 => 22720,
                8 => 69936,
                14 => 229936,
                22 => 820288,
                39 => 525312,
                48 => 820784,
                56 => 230176,
                62 => 70048,
                66 => 22784,
                70 => 21808,
                _ if !(5..66).contains(&b) => 20672,
                _ if !(9..62).contains(&b) => 61760,
                _ if !(15..56).contains(&b) => 197184,
                _ => 689232,
            }]
        };
        for (layer, size) in layers.into_iter().enumerate() {
            sizes.insert(format!("block{b}.layer{layer}.layer"), size);
        }
    }
    sizes.insert("block70.layer0.blend_scale".into(), 2);
    sizes
}

fn slice(data: &[u8], offset: usize, count: usize) -> Result<&[u8]> {
    data.get(offset..offset.checked_add(count).ok_or(Error::Invalid(INVALID))?)
        .ok_or(Error::Invalid(INVALID))
}
fn u64_at(data: &[u8], offset: usize) -> Result<usize> {
    let bytes = slice(data, offset, 8)?
        .try_into()
        .map_err(|_| Error::Invalid(INVALID))?;
    usize::try_from(u64::from_le_bytes(bytes)).map_err(|_| Error::Invalid(INVALID))
}

pub fn records(data: &[u8]) -> Result<BTreeMap<&str, &[u8]>> {
    require(u64_at(data, 0)? == data.len(), INVALID)?;
    let sizes = record_sizes();
    let mut result = BTreeMap::new();
    let mut cursor = 8;
    while cursor < data.len() {
        let name_len = u64_at(data, cursor)?;
        require((1..=4096).contains(&name_len), INVALID)?;
        let raw_name = slice(data, cursor + 8, name_len)?;
        require(raw_name.is_ascii(), INVALID)?;
        let name = std::str::from_utf8(raw_name).map_err(|_| Error::Invalid(INVALID))?;
        let span = u64_at(data, cursor + 8 + name_len)?;
        let body_start = cursor + 16 + name_len;
        let body = slice(data, body_start, span)?;
        let size = *sizes.get(name).ok_or(Error::Invalid(INVALID))?;
        require(
            span == size + 40
                && u64_at(body, 0)? == span
                && u64_at(body, 8)? == size
                && slice(body, 16, 4)? == 1_u32.to_le_bytes(),
            INVALID,
        )?;
        let trailer: Vec<u8> = [0, 0, 1, 0, size as u32 / 2]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        require(slice(body, 20 + size, 20)? == trailer, INVALID)?;
        require(
            result.insert(name, slice(body, 20, size)?).is_none(),
            INVALID,
        )?;
        cursor = body_start + span;
    }
    require(result.len() == sizes.len(), INVALID)?;
    Ok(result)
}

pub fn half(h: u16) -> f32 {
    let sign = u32::from(h & 0x8000) << 16;
    let exp = (h >> 10) & 31;
    let mant = h & 1023;
    if exp == 0 {
        let v = f32::from(mant) * (1.0 / 16777216.0);
        if sign != 0 { -v } else { v }
    } else {
        let exponent = if exp == 31 { 255 } else { u32::from(exp) + 112 };
        f32::from_bits(sign | (exponent << 23) | (u32::from(mant) << 13))
    }
}
fn halves(raw: &[u8]) -> Vec<f32> {
    assert_eq!(raw.len() % 2, 0);
    raw.as_chunks::<2>()
        .0
        .iter()
        .map(|v| half(u16::from_le_bytes(*v)))
        .collect()
}
fn floats(raw: &[u8]) -> Vec<f32> {
    assert_eq!(raw.len() % 4, 0);
    raw.as_chunks::<4>()
        .0
        .iter()
        .map(|v| f32::from_le_bytes(*v))
        .collect()
}
fn fp8(b: u8) -> f32 {
    let e = (b >> 3) & 15;
    let m = b & 7;
    let v = if e == 15 && m == 7 {
        f32::NAN
    } else if e == 0 {
        f32::from(m) / 512.0
    } else {
        f32::from_bits((u32::from(e) + 120) << 23 | u32::from(m) << 20)
    };
    if b & 128 != 0 { -v } else { v }
}
fn gather_bits(i: usize, positions: &[usize]) -> usize {
    positions
        .iter()
        .enumerate()
        .map(|(b, p)| ((i >> p) & 1) << b)
        .sum()
}
fn scatter_bits(i: usize, positions: &[usize]) -> usize {
    positions
        .iter()
        .enumerate()
        .map(|(b, p)| ((i >> b) & 1) << p)
        .sum()
}
fn axes(head: &[usize], tail: impl IntoIterator<Item = usize>) -> Vec<usize> {
    head.iter().copied().chain(tail).collect()
}
fn matrix(raw: &[u8], rb: &[usize], cb: &[usize]) -> Vec<f32> {
    assert_eq!(raw.len(), 1 << (rb.len() + cb.len()));
    let columns: Vec<_> = (0..1 << cb.len()).map(|i| scatter_bits(i, cb)).collect();
    (0..1 << rb.len())
        .flat_map(|r| {
            let row = scatter_bits(r, rb);
            columns.iter().map(move |&c| fp8(raw[row | c]))
        })
        .collect()
}
fn sparse(raw: &[u8], rows: usize, cols: usize, rb: &[usize], cb: &[usize]) -> Vec<f32> {
    let mut values = vec![0.0; rows * cols];
    for (i, &b) in raw.iter().enumerate() {
        values[gather_bits(i, rb) * cols + gather_bits(i, cb)] = fp8(b);
    }
    values
}
fn skip_order(ch: usize) -> Vec<usize> {
    (0..ch)
        .map(|s| (s / 16) * 16 + (s % 8) * 2 + (s % 16 / 8))
        .collect()
}
fn skip(raw: &[u8], order: &[usize]) -> Vec<f32> {
    let mut values = vec![0.0; order.len()];
    for (&index, v) in order.iter().zip(halves(raw)) {
        values[index] = v;
    }
    values
}
fn bias(raw: &[u8]) -> Vec<f32> {
    let mut values = vec![0.0; raw.len() / 2];
    for (i, v) in halves(raw).into_iter().enumerate() {
        let j = i % 4096;
        let q = gather_bits(j, &[5, 6, 10, 7, 1, 11]);
        let k = gather_bits(j, &[0, 3, 8, 4, 2, 9]);
        values[(i / 4096) * 4096 + q * 64 + k] = v;
    }
    values
}
fn interleaved(raw: &[u8], part: usize) -> Vec<u8> {
    assert_eq!(raw.len() % 3072, 0);
    raw.as_chunks::<3072>()
        .0
        .iter()
        .flat_map(|v| v[part * 1024..(part + 1) * 1024].iter().copied())
        .collect()
}

fn pre(raw: &[u8]) -> (Vec<f32>, Vec<u8>) {
    let mut mix = vec![0.0; 512];
    for (s, v) in halves(&raw[8208..9232]).into_iter().enumerate() {
        let ch = (s / 64) * 4 + (s / 32 % 2) + 2 * (s / 4 % 2);
        let feature = (s / 8 % 4) * 4 + s % 4;
        mix[ch * 16 + feature] = v;
    }
    (mix, [&raw[..8208], &raw[9232..]].concat())
}
fn post(raw: &[u8]) -> (Vec<u8>, Vec<f32>, Vec<f32>) {
    let ordinary = [&raw[..0x2050], &[0_u8; 16], &raw[0x20d0..0x5130]].concat();
    let scales = [
        skip(&raw[0x2050..0x2090], &C32),
        skip(&raw[0x2090..0x20d0], &C32),
    ]
    .concat();
    let mut head = vec![0.0; 512];
    for (i, v) in halves(&raw[0x5130..]).into_iter().enumerate() {
        head[gather_bits(i, &[2, 5, 6, 7]) * 32 + gather_bits(i, &[0, 1, 3, 4, 8])] = v;
    }
    (
        ordinary,
        scales,
        [&head[..32], &head[64..96], &head[128..160]].concat(),
    )
}
fn upsample(raw: &[u8], ch: usize) -> (Vec<f32>, Vec<u8>) {
    if ch == 32 {
        let body = [&raw[..0x2000], &raw[0x2800..0x2860], &raw[0x28a0..]].concat();
        let mat = matrix(&raw[0x2000..0x2800], &[3, 6, 7, 8, 9], &[1, 0, 4, 5, 2, 10]);
        let mut output = vec![0.0; 2048];
        for (i, source) in skip_order(32).into_iter().enumerate() {
            output[C32[i] * 64..(C32[i] + 1) * 64]
                .copy_from_slice(&mat[source * 64..(source + 1) * 64]);
        }
        output.extend(skip(&raw[0x2860..0x28a0], &C32));
        return (output, body);
    }
    let (n, begin, ffskip, qkv) = match ch {
        64 => (61760, 0x7000, 0x9000, 0x70a0),
        128 => (197184, 0x18000, 0x20000, 0x18120),
        256 => (689232, 0x58000, 0x78000, 0x58220),
        _ => unreachable!(),
    };
    let mut body = vec![0; n];
    body[..begin].copy_from_slice(&raw[..begin]);
    body[begin + 16..begin + 16 + 2 * ch].copy_from_slice(&raw[ffskip..ffskip + 2 * ch]);
    body[qkv..].copy_from_slice(&raw[ffskip + 4 * ch..]);
    let d = ch.ilog2() as usize;
    let mut weights = matrix(
        &raw[begin..ffskip],
        &axes(&[3], 6..d + 5),
        &axes(&[1, 0, 4, 5, 2], d + 5..2 * d + 1),
    );
    weights.extend(skip(
        &raw[ffskip + 2 * ch..ffskip + 4 * ch],
        &skip_order(ch),
    ));
    (weights, body)
}
fn downsample(raw: &[u8], ch: usize) -> Vec<f32> {
    if ch == 32 {
        return matrix(&raw[0x50b0..0x58b0], &[3, 6, 7, 8, 9, 10], &[0, 1, 4, 5, 2]);
    }
    let offset = match ch {
        64 => 0xf130,
        128 => 0x30230,
        256 => 0xa8440,
        _ => unreachable!(),
    };
    let d = ch.ilog2() as usize;
    matrix(
        &raw[offset..offset + 2 * ch * ch],
        &axes(&[3, 6, 7, 8, 9], 10..d + 6),
        &axes(&[1, 0, 4, 5, 2], d + 6..2 * d + 1),
    )
}
fn c32(raw: &[u8]) -> (Vec<f32>, Vec<f32>) {
    let mut ffn = vec![0.0; 512]; // unused ordinary C32 prefix, not learned coefficients
    ffn.extend(matrix(
        &raw[..4096],
        &[3, 6, 7, 8, 9, 10, 11],
        &[0, 1, 4, 5, 2],
    ));
    ffn.extend(matrix(
        &raw[4096..8192],
        &[6, 3, 7, 8, 9],
        &[1, 0, 4, 5, 2, 10, 11],
    ));
    ffn.extend(skip(&raw[0x2010..0x2050], &C32));
    let mut attention = Vec::new();
    for base in [0x2060, 0x2460, 0x2860, 0x4c70] {
        attention.extend(matrix(
            &raw[base..base + 1024],
            &[6, 3, 7, 8, 9],
            &[0, 1, 4, 5, 2],
        ));
    }
    attention.extend(bias(&raw[0x2c60..0x4c60]));
    attention.extend(floats(&raw[0x4c60..0x4c64]));
    attention.extend(skip(&raw[0x5070..0x50b0], &C32));
    (ffn, attention)
}
fn multi(raw: &[u8], ch: usize) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    let d = ch.ilog2() as usize;
    let hidden = 4 * ch;
    let b2 = hidden * ch;
    let b3 = b2 + 128 * ch;
    let mut ffn = sparse(
        &raw[..b2],
        hidden,
        ch,
        &axes(&[3, 6, 7, 8, 9, 10, 11], d + 7..2 * d + 2),
        &axes(&[1, 0, 4, 5, 2], 12..d + 7),
    );
    ffn.extend(sparse(
        &raw[b2..b3],
        ch,
        hidden,
        &axes(&[3, 6, 7, 8, 9], 12..d + 7),
        &axes(&[1, 0, 4, 5, 2, 10, 11], 12..d + 7),
    ));
    ffn.extend(sparse(
        &raw[b3..b3 + ch * ch],
        ch,
        ch,
        &axes(&[3, 6, 7, 8, 9], 10..d + 5),
        &axes(&[1, 0, 4, 5, 2], d + 5..2 * d),
    ));
    let (fs, scale, p, bias_start, at, base) = match ch {
        64 => (0x7010, 0xe0a0, 0xe0b0, 0xa0a0, 0xf0b0, 0x70a0),
        128 => (0x18010, 0x2c120, 0x2c130, 0x24120, 0x30130, 0x18120),
        256 => (0x58010, 0x98220, 0x98240, 0x88220, 0xa8240, 0x58220),
        _ => unreachable!(),
    };
    ffn.extend(skip(&raw[fs..fs + 2 * ch], &skip_order(ch)));
    let mut attention = Vec::new();
    let rb = axes(&[3, 6, 7, 8, 9], 10..d + 5);
    let cb = axes(&[1, 0, 4, 5, 2], d + 5..2 * d);
    for part in 0..3 {
        attention.extend(matrix(
            &interleaved(&raw[base..base + 3 * ch * ch], part),
            &rb,
            &cb,
        ));
    }
    attention.extend(matrix(&raw[p..p + ch * ch], &rb, &cb));
    attention.extend(bias(&raw[bias_start..scale]));
    attention.extend(floats(&raw[scale..scale + (ch / 32) * 4]));
    let components = attention.clone();
    attention.extend(skip(&raw[at..at + 2 * ch], &skip_order(ch)));
    (ffn, components, attention)
}
fn split(raws: [&[u8]; 4]) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    let mat = |raw: &[u8]| {
        matrix(
            raw,
            &[3, 6, 7, 8, 9, 10, 11, 12, 13],
            &[1, 0, 4, 5, 2, 14, 15, 16, 17],
        )
    };
    let mut fw = mat(&raws[0][..262144]);
    for (start, rb, cb) in [
        (
            0x40000,
            vec![3, 6, 7, 8, 9, 10, 11, 12],
            vec![1, 0, 4, 5, 2, 13],
        ),
        (
            0x60000,
            vec![3, 6, 7, 8, 9, 10],
            vec![1, 0, 4, 5, 2, 11, 12, 13],
        ),
    ] {
        for g in 0..8 {
            fw.extend(matrix(
                &raws[0][start + g * 16384..start + (g + 1) * 16384],
                &rb,
                &cb,
            ));
        }
    }
    let mut projection = mat(&raws[1][..262144]);
    projection.extend(skip(&raws[1][262144..], &skip_order(512)));
    let mut attention = Vec::new();
    for part in 0..3 {
        attention.extend(mat(&interleaved(&raws[2][..0xc0000], part)));
    }
    attention.extend(mat(&raws[3][..262144]));
    attention.extend(bias(&raws[2][0xc0000..0xe0000]));
    attention.extend(floats(&raws[2][0xe0000..]));
    attention.extend(skip(&raws[3][262144..], &skip_order(512)));
    (fw, projection, attention)
}
fn vit_matrix(raw: &[u8], inputs: usize, outputs: usize) -> Vec<f32> {
    let ib = inputs.ilog2() as usize;
    let ob = outputs.ilog2() as usize;
    matrix(
        raw,
        &axes(&[6, 3, 9, 7, 8], 10..ob + 5),
        &axes(&[0, 1, 2, 4, 5], ob + 5..ib + ob),
    )
}
fn vit_residual(raw: &[u8], inputs: usize) -> Vec<f32> {
    let mut values = vit_matrix(&raw[..inputs * 1024], inputs, 1024);
    let scales = halves(&raw[inputs * 1024..]);
    values.extend((0..1024).map(|i| scales[scatter_bits(i, &[0, 3, 4, 1, 2, 5, 6, 7, 8, 9])]));
    values
}

/// Emit one validated table at a time; peak memory does not scale with the cache.
/// Call only after `records` has validated the exact pinned set and lengths.
pub fn decode(
    records: &BTreeMap<&str, &[u8]>,
    mut emit: impl FnMut(String, Vec<f32>) -> Result<()>,
) -> Result<()> {
    let payload = |b, layer| records[format!("block{b}.layer{layer}.layer").as_str()];
    let (mix, pre_body) = pre(payload(0, 0));
    emit("block0-mix.audit.f32".into(), mix.clone())?;
    let (post_body, scales, head) = post(payload(70, 0));
    emit("post70-head.f32".into(), head)?;
    emit("post70-scales.f32".into(), scales)?;
    let mut bodies = BTreeMap::from([(0, pre_body), (70, post_body)]);
    for (b, ch) in [(48, 256), (56, 128), (62, 64), (66, 32)] {
        let (weights, body) = upsample(payload(b, 0), ch);
        emit(format!("block{b}-weights.f32"), weights)?;
        bodies.insert(b, body);
    }
    for b in (5..23).chain(48..66) {
        let ch = if !(9..62).contains(&b) {
            64
        } else if !(15..56).contains(&b) {
            128
        } else {
            256
        };
        let raw = bodies
            .get(&b)
            .map(Vec::as_slice)
            .unwrap_or_else(|| payload(b, 0));
        let (ffn, components, attention) = multi(raw, ch);
        emit(format!("block{b}-ffn.f32"), ffn)?;
        emit(
            format!("block{b}-attention-components.audit.f32"),
            components,
        )?;
        emit(format!("block{b}-attention.f32"), attention)?;
    }
    for b in (0..5).chain(66..71) {
        let raw = bodies
            .get(&b)
            .map(Vec::as_slice)
            .unwrap_or_else(|| payload(b, 0));
        let (mut ffn, attention) = c32(raw);
        if b == 0 {
            ffn[..512].copy_from_slice(&mix);
        }
        let prefix = if b == 70 {
            "post70".into()
        } else {
            format!("block{b}")
        };
        emit(format!("{prefix}-ffn.f32"), ffn)?;
        emit(format!("{prefix}-attention.f32"), attention)?;
    }
    for (b, ch) in [(4, 32), (8, 64), (14, 128), (22, 256)] {
        emit(format!("block{b}-ds.f32"), downsample(payload(b, 0), ch))?;
    }
    for b in (23..31).chain(40..48) {
        let (fw, projection, attention) =
            split([payload(b, 0), payload(b, 1), payload(b, 2), payload(b, 3)]);
        emit(format!("block{b}-ffwd.f32"), fw)?;
        emit(format!("block{b}-ffwd-projection.f32"), projection)?;
        emit(format!("block{b}-attention.f32"), attention)?;
    }
    emit(
        "head-matrix.f32".into(),
        matrix(
            &payload(30, 4)[..524288],
            &[3, 6, 7, 8, 9, 10, 11, 12, 13, 14],
            &[1, 0, 4, 5, 2, 15, 16, 17, 18],
        ),
    )?;
    for b in 31..39 {
        let raw = payload(b, 0);
        require(raw[4194304..].iter().all(|&b| b == 0), INVALID)?;
        emit(
            format!("block{b}-expand.f32"),
            vit_matrix(&raw[..4194304], 1024, 4096),
        )?;
        emit(
            format!("block{b}-contract.f32"),
            vit_residual(payload(b, 1), 4096),
        )?;
        let raw = payload(b, 2);
        let mut qkv = Vec::new();
        for part in 0..3 {
            qkv.extend(vit_matrix(&interleaved(&raw[128..], part), 1024, 1024));
        }
        qkv.extend(floats(&raw[..128]));
        emit(format!("block{b}-qkv.f32"), qkv)?;
        emit(
            format!("block{b}-projection.f32"),
            vit_residual(payload(b, 4), 1024),
        )?;
    }
    let raw = payload(39, 0);
    let mut weights = matrix(
        &raw[..524288],
        &[3, 6, 7, 8, 9, 10, 11, 12, 13],
        &[1, 0, 4, 5, 2, 14, 15, 16, 17, 18],
    );
    weights.extend(skip(&raw[524288..], &skip_order(512)));
    emit("decoder39-weights.f32".into(), weights)
}

pub fn bridge(inverse: bool) -> Vec<u8> {
    let mut result = Vec::with_capacity(655360 * 4);
    for t in 0..640_u32 {
        let p = if inverse {
            (t & !15) | ((t & 8) >> 3) | ((t & 7) << 1)
        } else {
            (t & !15) | ((t & 1) << 3) | ((t & 14) >> 1)
        };
        for c in 0..1024_u32 {
            let h = if inverse {
                (c & !31) | ((c & 1) << 1) | ((c & 2) >> 1) | ((c & 16) >> 2) | ((c & 12) << 1)
            } else {
                (c & !31) | ((c & 1) << 1) | ((c & 2) >> 1) | ((c & 4) << 2) | ((c & 24) >> 1)
            };
            result.extend_from_slice(&(p * 1024 + h).to_le_bytes());
        }
    }
    result
}
