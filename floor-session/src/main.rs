use hx_usb::Session;
use std::error::Error;
use std::io::{self, BufRead};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn drive_ticks(value: f32) -> Result<u8> {
    if !(0.0..=1.0).contains(&value) { return Err("Drive outside expected range".into()) }
    Ok((value * 100.0).round() as u8)
}

fn read_boost(session: &mut Session) -> Result<(bool, u8)> {
    let preset = session.read_preset()?;
    let mut blocks = preset.blocks();
    match (blocks.next(), blocks.next()) {
        (Some((2, slot)), None) if slot.model == Some(388) => {
            Ok((slot.enabled, drive_ticks(*slot.values.first().ok_or("Missing Drive")?)?))
        }
        _ => Err("Expected only Kinky Boost in position 2".into()),
    }
}

fn main() -> Result<()> {
    let devices = hx_usb::list()?;
    let [floor] = devices.as_slice() else { return Err("Expected one connected Floor".into()) };
    if floor.profile.product_id != 0x4248 { return Err("Expected the original Floor".into()) }
    let mut session = floor.open()?;
    let identity = session.preset_info()?;
    if session.profile.slot_label(identity.1) != "07C" || identity.2 != "New Preset" {
        return Err("Select the spare 07C New Preset".into());
    }
    let (mut enabled, mut drive) = read_boost(&mut session)?;
    if session.preset_info()? != identity { return Err("Preset changed during startup".into()) }
    println!("ready {enabled} 0 {drive}");

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for line in io::stdin().lock().lines().map_while(std::result::Result::ok) {
            if tx.send(line).is_err() { break }
        }
    });
    let start = Instant::now();
    let mut keepalive = start;
    let mut pending = None;
    let mut revision: u64 = 0;
    while start.elapsed() < Duration::from_secs(180) {
        let command = match rx.recv_timeout(Duration::from_millis(75)) {
            Ok(cmd) if cmd == "stop" => break,
            Ok(cmd) => Some(cmd),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        for (event, args) in session.poll_notifications()? {
            let field = |k| args.get(k).and_then(|v| v.as_i64());
            // Periodic idle tick, also documented in hx-usb's events-channel test.
            if event == 22 && field(82) == Some(0) && field(68) == Some(10) && field(121) == Some(27)
                && matches!(args.get(106), Some(hx_proto::msgpack::Value::Nil)) {
                continue;
            }
            let data = args.get(106);
            let detail = |k| data.and_then(|v| v.get(k));
            if field(82) != Some(0) || detail(98).and_then(|v| v.as_i64()) != Some(2) {
                return Err(format!("Floor context changed: {event} {args:?}").into());
            }
            match (event, field(68), field(121)) {
                (49, Some(5), Some(17)) => {
                    enabled = detail(59).and_then(|v| v.as_bool()).ok_or("Missing bypass state")?;
                }
                (30, Some(6), Some(20)) if detail(26).and_then(|v| v.as_i64()) == Some(0)
                    && detail(28).and_then(|v| v.as_i64()) == Some(0) => {
                    drive = drive_ticks(detail(119).and_then(|v| v.as_f32()).ok_or("Missing Drive value")?)?;
                }
                _ => return Err(format!("Another Floor change arrived: {event} {args:?}").into()),
            }
            revision += 1;
            println!("observed {enabled} {revision} {drive}");
            pending = None;
        }
        if let Some(sent) = pending {
            if Instant::now().duration_since(sent) >= Duration::from_secs(2) {
                if session.preset_info()? != identity { return Err("Preset changed".into()) }
                (enabled, drive) = read_boost(&mut session)?;
                revision += 1;
                println!("readback {enabled} {revision} {drive}");
                pending = None;
            }
        }
        if let Some(command) = command {
            let wanted = !enabled;
            let next_drive = command.strip_prefix(&format!("drive {revision} "))
                .and_then(|s| s.parse::<u8>().ok()).filter(|n| *n <= 100);
            if pending.is_some() || (next_drive.is_none() && command != format!("set {revision} {wanted}")) {
                println!("stale {enabled} {revision} {drive}");
            } else {
                if session.preset_info()? != identity { return Err("Preset changed before edit".into()) }
                if let Some(ticks) = next_drive {
                    session.set_param(2, 0, hx_proto::msgpack::Value::F32(f32::from(ticks) / 100.0))?;
                } else {
                    session.set_enabled(2, wanted)?;
                }
                pending = Some(Instant::now());
            }
        }
        if keepalive.elapsed() >= Duration::from_millis(700) {
            session.keepalive()?;
            keepalive = Instant::now();
        }
    }
    Ok(())
}
