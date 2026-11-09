use hx_usb::Session;
use serde_json::Value as Json;
use std::error::Error;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn samples(hex: &str, impulse: bool) -> Result<Vec<f32>> {
    if hex.len() != 2048 * 8 || !hex.bytes().all(|v| v.is_ascii_hexdigit()) {
        return Err("Invalid A2 Lite payload".into());
    }
    let mut data = Vec::with_capacity(2048);
    for word in hex.as_bytes().chunks_exact(8) {
        let mut bytes = [0; 4];
        for (byte, part) in bytes.iter_mut().zip(word.chunks_exact(2)) {
            *byte = u8::from_str_radix(std::str::from_utf8(part)?, 16)?;
        }
        data.push(f32::from_le_bytes(bytes));
    }
    if (data[..3] == [0.5, -0.5, 3.0]) == impulse || data.iter().any(|v| !v.is_finite()) {
        return Err("Samples do not match the selected storage type".into());
    }
    Ok(data)
}

pub fn install(session: &mut Session, cmd: &Json) -> Result<i64> {
    let number = cmd["captureSlot"]
        .as_i64()
        .filter(|n| (1..=128).contains(n))
        .ok_or("Invalid storage position")?;
    let slot = number - 1;
    let label = cmd["captureLabel"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 31)
        .ok_or("Invalid capture label")?;
    let wanted = samples(
        cmd["sampleBytes"]
            .as_str()
            .ok_or("Capture payload missing")?,
        cmd["op"] == "ir-install",
    )?;
    // The bridge serializes all operations. Recheck its reserved position on this
    // exclusive USB session immediately before issuing a single persistent write.
    let occupied = session.irs()?;
    match occupied.iter().find(|(i, _)| *i == slot) {
        Some((_, name)) if name == label => {} // A retry verifies the first upload in place.
        Some(_) => return Err("Storage position changed. Refresh the NAM library.".into()),
        None => {
            if occupied.iter().any(|(_, n)| n == label) || session.ir_descriptor(slot)?.is_some() {
                return Err("Floor directory changed. Refresh the NAM library.".into());
            }
            session.upload_ir(slot, label, &wanted)?;
        }
    }
    let (actual_label, actual) = session
        .read_ir(slot)?
        .ok_or("Floor returned an empty position after installation")?;
    if actual_label != label
        || actual.len() != wanted.len()
        || !actual
            .iter()
            .zip(&wanted)
            .all(|(a, b)| a.to_bits() == b.to_bits())
    {
        return Err("Capture readback differs. Installation could not be verified.".into());
    }
    Ok(number)
}

#[cfg(test)]
mod tests {
    #[test]
    fn words_preserve_signed_zero_and_reject_nonfinite() {
        let mut payload = String::from("0000003f000000bf00004040");
        payload.push_str(&"00000080".repeat(2045));
        assert_eq!(
            super::samples(&payload, false).unwrap()[3].to_bits(),
            (-0.0f32).to_bits()
        );
        assert!(super::samples(&payload.replace("00000080", "0000807f"), false).is_err());
        assert!(super::samples("00", false).is_err());
        assert!(super::samples(&payload, true).is_err());
        assert!(super::samples(&"00000000".repeat(2048), true).is_ok());
    }
}
