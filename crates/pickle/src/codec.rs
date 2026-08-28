//! SID-10 ByteCodecs: the encoding `ScalaSignature.bytes` uses to carry pickle
//! bytes through a Java `String` constant.
//!
//! Shared by the pickle writer (`scala-rs-backend`) and this crate's reader,
//! which is why it lives here rather than next to either one.

/// nsc `PickleFormat.MajorVersion`.
pub const MAJOR: u32 = 5;
/// nsc `PickleFormat.MinorVersion`.
pub const MINOR: u32 = 2;

// ---------------------------------------------------------------------------
// ByteCodecs (SID-10)
// ---------------------------------------------------------------------------

pub fn encode8to7(src: &[u8]) -> Vec<u8> {
    let srclen = src.len();
    let dstlen = (srclen * 8).div_ceil(7);
    let mut dst = vec![0u8; dstlen];
    let mut i = 0;
    let mut j = 0;
    while i + 6 < srclen {
        let mut inp = src[i] as i32;
        dst[j] = (inp & 0x7f) as u8;
        let mut out = inp >> 7;
        inp = src[i + 1] as i32;
        dst[j + 1] = (out | (inp << 1) & 0x7f) as u8;
        out = inp >> 6;
        inp = src[i + 2] as i32;
        dst[j + 2] = (out | (inp << 2) & 0x7f) as u8;
        out = inp >> 5;
        inp = src[i + 3] as i32;
        dst[j + 3] = (out | (inp << 3) & 0x7f) as u8;
        out = inp >> 4;
        inp = src[i + 4] as i32;
        dst[j + 4] = (out | (inp << 4) & 0x7f) as u8;
        out = inp >> 3;
        inp = src[i + 5] as i32;
        dst[j + 5] = (out | (inp << 5) & 0x7f) as u8;
        out = inp >> 2;
        inp = src[i + 6] as i32;
        dst[j + 6] = (out | (inp << 6) & 0x7f) as u8;
        out = inp >> 1;
        dst[j + 7] = out as u8;
        i += 7;
        j += 8;
    }
    if i < srclen {
        let mut inp = src[i] as i32;
        dst[j] = (inp & 0x7f) as u8;
        j += 1;
        let mut out = inp >> 7;
        if i + 1 < srclen {
            inp = src[i + 1] as i32;
            dst[j] = (out | (inp << 1) & 0x7f) as u8;
            j += 1;
            out = inp >> 6;
            if i + 2 < srclen {
                inp = src[i + 2] as i32;
                dst[j] = (out | (inp << 2) & 0x7f) as u8;
                j += 1;
                out = inp >> 5;
                if i + 3 < srclen {
                    inp = src[i + 3] as i32;
                    dst[j] = (out | (inp << 3) & 0x7f) as u8;
                    j += 1;
                    out = inp >> 4;
                    if i + 4 < srclen {
                        inp = src[i + 4] as i32;
                        dst[j] = (out | (inp << 4) & 0x7f) as u8;
                        j += 1;
                        out = inp >> 3;
                        if i + 5 < srclen {
                            inp = src[i + 5] as i32;
                            dst[j] = (out | (inp << 5) & 0x7f) as u8;
                            j += 1;
                            out = inp >> 2;
                        }
                    }
                }
            }
        }
        if j < dstlen {
            dst[j] = out as u8;
        }
    }
    dst
}

/// nsc `ScalaSigBytes.mapToNextModSevenBits`: 0x7f → 0, else +1.
/// Zero bytes are stored as modified UTF-8 `C0 80` in the classfile Utf8,
/// which `ByteCodecs.decode` / [`regenerate_zero`] map back to 0x7f.
pub fn avoid_zero(src: &[u8]) -> Vec<u8> {
    src.iter()
        .map(|&inp| if inp == 0x7f { 0 } else { inp.wrapping_add(1) })
        .collect()
}

pub fn encode_bytes(src: &[u8]) -> Vec<u8> {
    avoid_zero(&encode8to7(src))
}

/// Encode pickle bytes as the Java String stored in `ScalaSignature.bytes`
/// (latin-1 chars, later written as modified UTF-8 in the classfile).
pub fn encode_to_annotation_string(src: &[u8]) -> String {
    encode_bytes(src).into_iter().map(char::from).collect()
}

pub fn regenerate_zero(src: &mut [u8]) -> usize {
    let srclen = src.len();
    let mut i = 0;
    let mut j = 0;
    while i < srclen {
        let inp = src[i] as u32;
        if inp == 0xc0 && i + 1 < srclen && (src[i + 1] as u32) == 0x80 {
            src[j] = 0x7f;
            i += 2;
        } else if inp == 0 {
            src[j] = 0x7f;
            i += 1;
        } else {
            src[j] = (inp as u8).wrapping_sub(1);
            i += 1;
        }
        j += 1;
    }
    j
}

pub fn decode7to8(src: &mut [u8], srclen: usize) -> usize {
    let mut i = 0;
    let mut j = 0;
    // Inverse of encode8to7's `(srclen * 8 + 6) / 7`. The nsc formula
    // `(srclen * 7 + 7) / 8` rounds up and leaves a padding 0 that would
    // look like another pickle entry.
    let dstlen = (srclen * 7) / 8;
    while i + 7 < srclen {
        let mut out = src[i] as i32;
        let mut inp = src[i + 1] as i32;
        src[j] = (out | (inp & 0x01) << 7) as u8;
        out = inp >> 1;
        inp = src[i + 2] as i32;
        src[j + 1] = (out | (inp & 0x03) << 6) as u8;
        out = inp >> 2;
        inp = src[i + 3] as i32;
        src[j + 2] = (out | (inp & 0x07) << 5) as u8;
        out = inp >> 3;
        inp = src[i + 4] as i32;
        src[j + 3] = (out | (inp & 0x0f) << 4) as u8;
        out = inp >> 4;
        inp = src[i + 5] as i32;
        src[j + 4] = (out | (inp & 0x1f) << 3) as u8;
        out = inp >> 5;
        inp = src[i + 6] as i32;
        src[j + 5] = (out | (inp & 0x3f) << 2) as u8;
        out = inp >> 6;
        inp = src[i + 7] as i32;
        src[j + 6] = (out | inp << 1) as u8;
        i += 8;
        j += 7;
    }
    if i < srclen {
        let mut out = src[i] as i32;
        if i + 1 < srclen {
            let mut inp = src[i + 1] as i32;
            src[j] = (out | (inp & 0x01) << 7) as u8;
            j += 1;
            out = inp >> 1;
            if i + 2 < srclen {
                inp = src[i + 2] as i32;
                src[j] = (out | (inp & 0x03) << 6) as u8;
                j += 1;
                out = inp >> 2;
                if i + 3 < srclen {
                    inp = src[i + 3] as i32;
                    src[j] = (out | (inp & 0x07) << 5) as u8;
                    j += 1;
                    out = inp >> 3;
                    if i + 4 < srclen {
                        inp = src[i + 4] as i32;
                        src[j] = (out | (inp & 0x0f) << 4) as u8;
                        j += 1;
                        out = inp >> 4;
                        if i + 5 < srclen {
                            inp = src[i + 5] as i32;
                            src[j] = (out | (inp & 0x1f) << 3) as u8;
                            j += 1;
                            out = inp >> 5;
                            if i + 6 < srclen {
                                inp = src[i + 6] as i32;
                                src[j] = (out | (inp & 0x3f) << 2) as u8;
                                j += 1;
                            }
                        }
                    }
                }
            }
        }
        if j < dstlen {
            src[j] = out as u8;
        }
    }
    dstlen
}

/// Decode a `ScalaSignature.bytes` Java String (latin-1 chars) to pickle bytes.
pub fn decode_annotation_string(s: &str) -> Vec<u8> {
    let mut buf: Vec<u8> = s.chars().map(|c| c as u8).collect();
    let len = regenerate_zero(&mut buf);
    let n = decode7to8(&mut buf, len);
    buf.truncate(n.min(buf.len()));
    buf
}
