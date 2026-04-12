use std::ops::Range;

use thiserror::Error;

pub type Mp4Result<T> = Result<T, Mp4Error>;

const BOX_ENCA: [u8; 4] = *b"enca";
const BOX_FRMA: [u8; 4] = *b"frma";
const BOX_MDAT: [u8; 4] = *b"mdat";
const BOX_MDIA: [u8; 4] = *b"mdia";
const BOX_MINF: [u8; 4] = *b"minf";
const BOX_MOOF: [u8; 4] = *b"moof";
const BOX_MOOV: [u8; 4] = *b"moov";
const BOX_MP4A: [u8; 4] = *b"mp4a";
const BOX_PSSH: [u8; 4] = *b"pssh";
const BOX_SAIO: [u8; 4] = *b"saio";
const BOX_SAIZ: [u8; 4] = *b"saiz";
const BOX_SBGP: [u8; 4] = *b"sbgp";
const BOX_SENC: [u8; 4] = *b"senc";
const BOX_SGPD: [u8; 4] = *b"sgpd";
const BOX_SINF: [u8; 4] = *b"sinf";
const BOX_STBL: [u8; 4] = *b"stbl";
const BOX_STSD: [u8; 4] = *b"stsd";
const BOX_TFHD: [u8; 4] = *b"tfhd";
const BOX_TRAF: [u8; 4] = *b"traf";
const BOX_TRAK: [u8; 4] = *b"trak";
const BOX_TRUN: [u8; 4] = *b"trun";

#[derive(Debug, Error)]
pub enum Mp4Error {
    #[error("invalid MP4 structure: {0}")]
    Invalid(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Mp4Box {
    pub offset: usize,
    pub size: usize,
    pub box_type: [u8; 4],
    pub header_size: usize,
}

impl Mp4Box {
    pub fn payload_start(&self) -> usize {
        self.offset + self.header_size
    }

    pub fn payload_end(&self) -> usize {
        self.offset + self.size
    }
}

pub fn iter_boxes(data: &[u8], start: usize, end: usize) -> Mp4Result<Vec<Mp4Box>> {
    if end > data.len() {
        return Err(Mp4Error::Invalid(format!(
            "box iterator end {end} exceeds data length {}",
            data.len()
        )));
    }
    if start > end {
        return Err(Mp4Error::Invalid(format!(
            "box iterator start {start} is greater than end {end}"
        )));
    }

    let mut offset = start;
    let mut out = Vec::new();
    while offset + 8 <= end {
        let size32 = read_u32(data, offset)? as usize;
        let box_type = read_fourcc(data, offset + 4)?;
        let mut box_size = size32;
        let mut header_size = 8usize;

        if size32 == 1 {
            let size64 = read_u64(data, offset + 8)?;
            box_size = usize::try_from(size64).map_err(|_| {
                Mp4Error::Invalid(format!(
                    "box {} at {offset} has 64-bit size that cannot fit usize: {size64}",
                    fourcc_to_string(box_type)
                ))
            })?;
            header_size = 16;
        } else if size32 == 0 {
            box_size = end - offset;
        }

        if box_size == 0 {
            return Err(Mp4Error::Invalid(format!(
                "box {} at {offset} has zero size",
                fourcc_to_string(box_type)
            )));
        }

        let box_end = checked_add(offset, box_size, "box end overflow")?;
        if box_end > end {
            return Err(Mp4Error::Invalid(format!(
                "box {} at {offset} exceeds parent end: box_end={box_end}, end={end}",
                fourcc_to_string(box_type)
            )));
        }

        out.push(Mp4Box {
            offset,
            size: box_size,
            box_type,
            header_size,
        });
        offset = box_end;
    }

    Ok(out)
}

#[allow(dead_code)]
pub fn find_child_box(
    data: &[u8],
    start: usize,
    size: usize,
    wanted_type: &str,
) -> Mp4Result<Option<Mp4Box>> {
    let wanted = parse_fourcc(wanted_type)?;
    find_child_box_fourcc(data, start, size, wanted)
}

pub fn collect_sample_slices(fragment: &[u8]) -> Mp4Result<Vec<Range<usize>>> {
    let moof = find_child_box_fourcc(fragment, 0, fragment.len(), BOX_MOOF)?
        .ok_or_else(|| Mp4Error::Invalid("fragment is missing moof".into()))?;
    let mdat = find_child_box_fourcc(fragment, 0, fragment.len(), BOX_MDAT)?
        .ok_or_else(|| Mp4Error::Invalid("fragment is missing mdat".into()))?;

    let moof_payload_start = moof.payload_start();
    let moof_payload_end = moof.payload_end();

    let mut ranges = Vec::new();
    let mut current_data_offset: Option<usize> = None;
    for traf in iter_boxes(fragment, moof_payload_start, moof_payload_end)?
        .into_iter()
        .filter(|item| item.box_type == BOX_TRAF)
    {
        let tfhd = find_child_box_fourcc(fragment, traf.payload_start(), traf.size - 8, BOX_TFHD)?
            .ok_or_else(|| Mp4Error::Invalid("traf is missing tfhd".into()))?;

        let tfhd_flags = read_u32(fragment, tfhd.offset + 8)? & 0x00FF_FFFF;
        let mut cursor = checked_add(tfhd.offset, 12, "tfhd cursor overflow")?;
        cursor = checked_add(cursor, 4, "tfhd track id cursor overflow")?;
        if tfhd_flags & 0x01 != 0 {
            cursor = checked_add(cursor, 8, "tfhd base data offset cursor overflow")?;
        }
        if tfhd_flags & 0x02 != 0 {
            cursor = checked_add(cursor, 4, "tfhd sample description cursor overflow")?;
        }
        if tfhd_flags & 0x08 != 0 {
            cursor = checked_add(cursor, 4, "tfhd default sample duration cursor overflow")?;
        }
        let default_sample_size = if tfhd_flags & 0x10 != 0 {
            let value = read_u32(fragment, cursor)?;
            cursor = checked_add(cursor, 4, "tfhd default sample size cursor overflow")?;
            Some(value as usize)
        } else {
            None
        };
        if tfhd_flags & 0x20 != 0 {
            let _ = checked_add(cursor, 4, "tfhd default sample flags cursor overflow")?;
        }

        for trun in iter_boxes(fragment, traf.payload_start(), traf.payload_end())?
            .into_iter()
            .filter(|item| item.box_type == BOX_TRUN)
        {
            let trun_flags = read_u32(fragment, trun.offset + 8)? & 0x00FF_FFFF;
            let sample_count = read_u32(fragment, trun.offset + 12)? as usize;
            let mut trun_cursor = checked_add(trun.offset, 16, "trun cursor overflow")?;

            let mut sample_offset = if trun_flags & 0x01 != 0 {
                let relative = read_i32(fragment, trun_cursor)? as i64;
                trun_cursor = checked_add(trun_cursor, 4, "trun data offset cursor overflow")?;
                let absolute = moof.offset as i64 + relative;
                if absolute < 0 {
                    return Err(Mp4Error::Invalid(format!(
                        "trun data offset resolved to negative value: {absolute}"
                    )));
                }
                absolute as usize
            } else if let Some(offset) = current_data_offset {
                offset
            } else {
                checked_add(mdat.offset, mdat.header_size, "mdat data offset overflow")?
            };

            if trun_flags & 0x04 != 0 {
                trun_cursor =
                    checked_add(trun_cursor, 4, "trun first sample flags cursor overflow")?;
            }

            for _ in 0..sample_count {
                if trun_flags & 0x100 != 0 {
                    trun_cursor = checked_add(trun_cursor, 4, "trun sample duration overflow")?;
                }

                let sample_size = if trun_flags & 0x200 != 0 {
                    let value = read_u32(fragment, trun_cursor)? as usize;
                    trun_cursor = checked_add(trun_cursor, 4, "trun sample size overflow")?;
                    value
                } else {
                    default_sample_size.ok_or_else(|| {
                        Mp4Error::Invalid(
                            "trun omitted sample sizes and tfhd omitted default sample size".into(),
                        )
                    })?
                };

                if trun_flags & 0x400 != 0 {
                    trun_cursor = checked_add(trun_cursor, 4, "trun sample flags overflow")?;
                }
                if trun_flags & 0x800 != 0 {
                    trun_cursor = checked_add(trun_cursor, 4, "trun cts offset overflow")?;
                }

                let sample_end = checked_add(sample_offset, sample_size, "sample end overflow")?;
                if sample_end > fragment.len() {
                    return Err(Mp4Error::Invalid(
                        "sample range exceeded fragment size".into(),
                    ));
                }
                ranges.push(sample_offset..sample_end);
                sample_offset = sample_end;
            }

            current_data_offset = Some(sample_offset);
        }
    }

    if ranges.is_empty() {
        return Err(Mp4Error::Invalid(
            "fragment did not expose any decryptable samples".into(),
        ));
    }

    Ok(ranges)
}

pub fn sanitize_audio_sample_entry(entry_box: &[u8]) -> Mp4Result<Vec<u8>> {
    if entry_box.len() < 8 {
        return Err(Mp4Error::Invalid("audio sample entry is too short".into()));
    }

    let entry_type = read_fourcc(entry_box, 4)?;
    if entry_type != BOX_ENCA {
        return Ok(entry_box.to_vec());
    }

    if entry_box.len() < 36 {
        return Err(Mp4Error::Invalid(
            "enca sample entry is too short for AudioSampleEntry fields".into(),
        ));
    }

    let audio_prefix = &entry_box[8..36];
    let mut clear_entry_type = BOX_MP4A;
    let mut kept_children = Vec::new();
    for child in iter_boxes(entry_box, 36, entry_box.len())? {
        let child_bytes = &entry_box[child.offset..child.offset + child.size];
        if child.box_type == BOX_SINF {
            if let Some(frma) =
                find_child_box_fourcc(child_bytes, 8, child_bytes.len() - 8, BOX_FRMA)?
            {
                let frma_type_offset = checked_add(frma.offset, 8, "frma payload offset overflow")?;
                clear_entry_type = read_fourcc(child_bytes, frma_type_offset)?;
            }
            continue;
        }
        kept_children.extend_from_slice(child_bytes);
    }

    let mut payload = Vec::with_capacity(audio_prefix.len() + kept_children.len());
    payload.extend_from_slice(audio_prefix);
    payload.extend_from_slice(&kept_children);
    rebuild_box(clear_entry_type, &payload)
}

pub fn sanitize_stsd_box(stsd_box: &[u8]) -> Mp4Result<Vec<u8>> {
    if stsd_box.len() < 16 {
        return Err(Mp4Error::Invalid("stsd box is too short".into()));
    }

    let version_and_flags = &stsd_box[8..12];
    let entries = iter_boxes(stsd_box, 16, stsd_box.len())?;
    let first_entry = entries
        .first()
        .ok_or_else(|| Mp4Error::Invalid("stsd did not contain any sample entries".into()))?;
    let first_entry_bytes = &stsd_box[first_entry.offset..first_entry.offset + first_entry.size];
    let sanitized = sanitize_audio_sample_entry(first_entry_bytes)?;

    let mut payload = Vec::with_capacity(8 + sanitized.len());
    payload.extend_from_slice(version_and_flags);
    payload.extend_from_slice(&1u32.to_be_bytes());
    payload.extend_from_slice(&sanitized);
    rebuild_box(BOX_STSD, &payload)
}

pub fn sanitize_stbl_box(stbl_box: &[u8]) -> Mp4Result<Vec<u8>> {
    sanitize_nested_box(stbl_box, BOX_STBL, |child| match child.box_type {
        BOX_SGPD | BOX_SBGP => Ok(None),
        BOX_STSD => Ok(Some(sanitize_stsd_box(child.bytes)?)),
        _ => Ok(Some(child.bytes.to_vec())),
    })
}

pub fn sanitize_minf_box(minf_box: &[u8]) -> Mp4Result<Vec<u8>> {
    sanitize_nested_box(minf_box, BOX_MINF, |child| match child.box_type {
        BOX_STBL => Ok(Some(sanitize_stbl_box(child.bytes)?)),
        _ => Ok(Some(child.bytes.to_vec())),
    })
}

pub fn sanitize_mdia_box(mdia_box: &[u8]) -> Mp4Result<Vec<u8>> {
    sanitize_nested_box(mdia_box, BOX_MDIA, |child| match child.box_type {
        BOX_MINF => Ok(Some(sanitize_minf_box(child.bytes)?)),
        _ => Ok(Some(child.bytes.to_vec())),
    })
}

pub fn sanitize_trak_box(trak_box: &[u8]) -> Mp4Result<Vec<u8>> {
    sanitize_nested_box(trak_box, BOX_TRAK, |child| match child.box_type {
        BOX_MDIA => Ok(Some(sanitize_mdia_box(child.bytes)?)),
        _ => Ok(Some(child.bytes.to_vec())),
    })
}

pub fn sanitize_moov_box(moov_box: &[u8]) -> Mp4Result<Vec<u8>> {
    sanitize_nested_box(moov_box, BOX_MOOV, |child| match child.box_type {
        BOX_TRAK => Ok(Some(sanitize_trak_box(child.bytes)?)),
        _ => Ok(Some(child.bytes.to_vec())),
    })
}

pub fn patch_trun_data_offset(trun_box: &[u8], delta: usize) -> Mp4Result<Vec<u8>> {
    if delta == 0 {
        return Ok(trun_box.to_vec());
    }

    if trun_box.len() < 16 {
        return Err(Mp4Error::Invalid("trun box is too short".into()));
    }

    let mut patched = trun_box.to_vec();
    let flags = read_u32(&patched, 8)? & 0x00FF_FFFF;
    if flags & 0x01 == 0 {
        return Ok(patched);
    }

    let current = read_i32(&patched, 16)? as i64;
    let delta_i64 = i64::try_from(delta)
        .map_err(|_| Mp4Error::Invalid(format!("trun delta too large: {delta}")))?;
    let new_value = current - delta_i64;
    if new_value < i32::MIN as i64 || new_value > i32::MAX as i64 {
        return Err(Mp4Error::Invalid(format!(
            "patched trun data offset out of i32 range: {new_value}"
        )));
    }
    patched[16..20].copy_from_slice(&(new_value as i32).to_be_bytes());
    Ok(patched)
}

pub fn patch_tfhd_sample_description_index(tfhd_box: &[u8]) -> Mp4Result<Vec<u8>> {
    if tfhd_box.len() < 16 {
        return Err(Mp4Error::Invalid("tfhd box is too short".into()));
    }

    let mut patched = tfhd_box.to_vec();
    let flags = read_u32(&patched, 8)? & 0x00FF_FFFF;
    if flags & 0x02 == 0 {
        return Ok(patched);
    }

    let mut cursor = 12usize;
    cursor = checked_add(cursor, 4, "tfhd track id cursor overflow")?;
    if flags & 0x01 != 0 {
        cursor = checked_add(cursor, 8, "tfhd base data offset cursor overflow")?;
    }
    ensure_range(&patched, cursor, 4)?;
    patched[cursor..cursor + 4].copy_from_slice(&1u32.to_be_bytes());
    Ok(patched)
}

pub fn strip_traf_encryption_boxes(traf_box: &[u8]) -> Mp4Result<(Vec<u8>, usize)> {
    let mut removed = 0usize;
    let mut kept_payload = Vec::new();
    for child in iter_boxes(traf_box, 8, traf_box.len())? {
        let child_bytes = &traf_box[child.offset..child.offset + child.size];
        match child.box_type {
            BOX_SENC | BOX_SAIZ | BOX_SAIO | BOX_SGPD | BOX_SBGP => {
                removed = checked_add(removed, child.size, "removed byte count overflow")?;
            }
            _ => kept_payload.extend_from_slice(child_bytes),
        }
    }
    Ok((rebuild_box(BOX_TRAF, &kept_payload)?, removed))
}

pub fn patch_traf_data_offsets(traf_box: &[u8], delta: usize) -> Mp4Result<Vec<u8>> {
    let mut payload = Vec::new();
    for child in iter_boxes(traf_box, 8, traf_box.len())? {
        let child_bytes = &traf_box[child.offset..child.offset + child.size];
        let patched = if child.box_type == BOX_TRUN {
            patch_trun_data_offset(child_bytes, delta)?
        } else if child.box_type == BOX_TFHD {
            patch_tfhd_sample_description_index(child_bytes)?
        } else {
            child_bytes.to_vec()
        };
        payload.extend_from_slice(&patched);
    }
    rebuild_box(BOX_TRAF, &payload)
}

pub fn sanitize_moof_box(moof_box: &[u8]) -> Mp4Result<Vec<u8>> {
    let mut removed = 0usize;
    let mut children: Vec<(Mp4Box, Vec<u8>)> = Vec::new();
    for child in iter_boxes(moof_box, 8, moof_box.len())? {
        let child_bytes = &moof_box[child.offset..child.offset + child.size];
        if child.box_type == BOX_TRAF {
            let (stripped, removed_bytes) = strip_traf_encryption_boxes(child_bytes)?;
            removed = checked_add(removed, removed_bytes, "moof removed byte count overflow")?;
            children.push((child, stripped));
        } else if child.box_type == BOX_PSSH {
            removed = checked_add(removed, child.size, "moof removed byte count overflow")?;
        } else {
            children.push((child, child_bytes.to_vec()));
        }
    }

    let mut payload = Vec::new();
    for (child, bytes) in children {
        if child.box_type == BOX_TRAF {
            payload.extend_from_slice(&patch_traf_data_offsets(&bytes, removed)?);
        } else {
            payload.extend_from_slice(&bytes);
        }
    }
    rebuild_box(BOX_MOOF, &payload)
}

pub fn sanitize_fragment(fragment: &[u8]) -> Mp4Result<Vec<u8>> {
    let mut out = Vec::new();
    for item in iter_boxes(fragment, 0, fragment.len())? {
        let bytes = &fragment[item.offset..item.offset + item.size];
        if item.box_type == BOX_MOOF {
            out.extend_from_slice(&sanitize_moof_box(bytes)?);
        } else {
            out.extend_from_slice(bytes);
        }
    }
    Ok(out)
}

pub fn sanitize_init_segment(init_data: &[u8]) -> Mp4Result<Vec<u8>> {
    let mut out = Vec::new();
    for item in iter_boxes(init_data, 0, init_data.len())? {
        let bytes = &init_data[item.offset..item.offset + item.size];
        if item.box_type == BOX_MOOV {
            out.extend_from_slice(&sanitize_moov_box(bytes)?);
        } else {
            out.extend_from_slice(bytes);
        }
    }
    Ok(out)
}

pub fn make_adts_header(frame_size: usize, sample_rate: u32, channels: u8) -> Mp4Result<[u8; 7]> {
    make_adts_header_with_profile(frame_size, sample_rate, channels, 2)
}

pub fn make_adts_header_with_profile(
    frame_size: usize,
    sample_rate: u32,
    channels: u8,
    profile: u8,
) -> Mp4Result<[u8; 7]> {
    if profile == 0 {
        return Err(Mp4Error::Invalid(
            "AAC profile must be greater than zero".into(),
        ));
    }
    if channels > 7 {
        return Err(Mp4Error::Invalid(format!(
            "AAC channel count out of ADTS range: {channels}"
        )));
    }
    let sample_rate_index = aac_sample_rate_index(sample_rate)
        .ok_or_else(|| Mp4Error::Invalid(format!("unsupported AAC sample rate: {sample_rate}")))?;

    let full_size = frame_size
        .checked_add(7)
        .ok_or_else(|| Mp4Error::Invalid("AAC frame size overflow".into()))?;
    if full_size > 0x1FFF {
        return Err(Mp4Error::Invalid(format!(
            "AAC frame too large for ADTS header: {full_size}"
        )));
    }

    let profile_bits = profile - 1;
    Ok([
        0xFF,
        0xF1,
        ((profile_bits & 0x03) << 6) | ((sample_rate_index & 0x0F) << 2) | ((channels >> 2) & 0x01),
        ((channels & 0x03) << 6) | (((full_size >> 11) & 0x03) as u8),
        ((full_size >> 3) & 0xFF) as u8,
        (((full_size & 0x07) << 5) as u8) | 0x1F,
        0xFC,
    ])
}

fn sanitize_nested_box<F>(input: &[u8], expected_type: [u8; 4], mut map: F) -> Mp4Result<Vec<u8>>
where
    F: FnMut(NestedChild<'_>) -> Mp4Result<Option<Vec<u8>>>,
{
    if input.len() < 8 {
        return Err(Mp4Error::Invalid(format!(
            "{} box is too short",
            fourcc_to_string(expected_type)
        )));
    }
    let actual_type = read_fourcc(input, 4)?;
    if actual_type != expected_type {
        return Err(Mp4Error::Invalid(format!(
            "expected {} box, got {}",
            fourcc_to_string(expected_type),
            fourcc_to_string(actual_type)
        )));
    }

    let mut payload = Vec::new();
    for child in iter_boxes(input, 8, input.len())? {
        let bytes = &input[child.offset..child.offset + child.size];
        if let Some(out) = map(NestedChild {
            box_type: child.box_type,
            bytes,
        })? {
            payload.extend_from_slice(&out);
        }
    }
    rebuild_box(expected_type, &payload)
}

fn find_child_box_fourcc(
    data: &[u8],
    start: usize,
    size: usize,
    wanted_type: [u8; 4],
) -> Mp4Result<Option<Mp4Box>> {
    let end = checked_add(start, size, "child box end overflow")?;
    for item in iter_boxes(data, start, end)? {
        if item.box_type == wanted_type {
            return Ok(Some(item));
        }
    }
    Ok(None)
}

fn rebuild_box(box_type: [u8; 4], payload: &[u8]) -> Mp4Result<Vec<u8>> {
    let total_size = payload
        .len()
        .checked_add(8)
        .ok_or_else(|| Mp4Error::Invalid("box size overflow".into()))?;
    if total_size >= (1u64 << 32) as usize {
        return Err(Mp4Error::Invalid(format!(
            "box {} is too large for 32-bit size: {total_size}",
            fourcc_to_string(box_type)
        )));
    }

    let mut out = Vec::with_capacity(total_size);
    out.extend_from_slice(&(total_size as u32).to_be_bytes());
    out.extend_from_slice(&box_type);
    out.extend_from_slice(payload);
    Ok(out)
}

#[allow(dead_code)]
fn parse_fourcc(value: &str) -> Mp4Result<[u8; 4]> {
    let bytes = value.as_bytes();
    if bytes.len() != 4 {
        return Err(Mp4Error::Invalid(format!(
            "fourcc must be exactly 4 bytes: {value}"
        )));
    }
    Ok([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn read_u32(data: &[u8], offset: usize) -> Mp4Result<u32> {
    ensure_range(data, offset, 4)?;
    Ok(u32::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]))
}

fn read_i32(data: &[u8], offset: usize) -> Mp4Result<i32> {
    Ok(read_u32(data, offset)? as i32)
}

fn read_u64(data: &[u8], offset: usize) -> Mp4Result<u64> {
    ensure_range(data, offset, 8)?;
    Ok(u64::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
        data[offset + 4],
        data[offset + 5],
        data[offset + 6],
        data[offset + 7],
    ]))
}

fn read_fourcc(data: &[u8], offset: usize) -> Mp4Result<[u8; 4]> {
    ensure_range(data, offset, 4)?;
    Ok([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

fn ensure_range(data: &[u8], offset: usize, length: usize) -> Mp4Result<()> {
    let end = checked_add(offset, length, "range overflow")?;
    if end > data.len() {
        return Err(Mp4Error::Invalid(format!(
            "buffer underflow: need {length} bytes at offset {offset}, len={}",
            data.len()
        )));
    }
    Ok(())
}

fn checked_add(lhs: usize, rhs: usize, context: &str) -> Mp4Result<usize> {
    lhs.checked_add(rhs)
        .ok_or_else(|| Mp4Error::Invalid(context.to_owned()))
}

fn fourcc_to_string(value: [u8; 4]) -> String {
    String::from_utf8_lossy(&value).into_owned()
}

fn aac_sample_rate_index(sample_rate: u32) -> Option<u8> {
    match sample_rate {
        96_000 => Some(0),
        88_200 => Some(1),
        64_000 => Some(2),
        48_000 => Some(3),
        44_100 => Some(4),
        32_000 => Some(5),
        24_000 => Some(6),
        22_050 => Some(7),
        16_000 => Some(8),
        12_000 => Some(9),
        11_025 => Some(10),
        8_000 => Some(11),
        7_350 => Some(12),
        _ => None,
    }
}

struct NestedChild<'a> {
    box_type: [u8; 4],
    bytes: &'a [u8],
}

#[cfg(test)]
mod tests {
    use super::{iter_boxes, make_adts_header};

    #[test]
    fn iter_boxes_reads_simple_payload() {
        let mut data = Vec::new();
        data.extend_from_slice(&8u32.to_be_bytes());
        data.extend_from_slice(b"ftyp");
        data.extend_from_slice(&12u32.to_be_bytes());
        data.extend_from_slice(b"free");
        data.extend_from_slice(&[1, 2, 3, 4]);

        let boxes = iter_boxes(&data, 0, data.len()).expect("iter_boxes should parse");
        assert_eq!(boxes.len(), 2);
        assert_eq!(boxes[0].box_type, *b"ftyp");
        assert_eq!(boxes[1].box_type, *b"free");
        assert_eq!(boxes[1].size, 12);
    }

    #[test]
    fn adts_header_matches_expected_bits() {
        let header = make_adts_header(1024, 44_100, 2).expect("adts header should build");
        assert_eq!(header[0], 0xFF);
        assert_eq!(header[1], 0xF1);
    }
}
