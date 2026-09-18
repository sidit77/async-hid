// devd(8) event parser for hidraw attach/detach notifications.
//
// devd sends newline-delimited text on /var/run/devd.pipe (SOCK_STREAM).
// Lines beginning with '+' or '-' announce device attach and detach:
//
//   +hidraw1 at index=255 page=0x0000 usage=0x0000 ... on hidbus1
//   -hidraw1 at index=255 page=0x0000 usage=0x0000 ... on hidbus1
//
// We match only names of the form `hidraw<digits>` and ignore other events
// (usbhid, hidbus, hkbd, ...) — enumerate() only returns /dev/hidraw*, so
// consumers watch that surface only.

use std::path::PathBuf;

/// One event as parsed from devd's stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedEvent {
    Attach(PathBuf),
    Detach(PathBuf),
}

/// Parse one line. Returns None for lines we don't care about.
pub fn parse_line(line: &str) -> Option<ParsedEvent> {
    let (kind, rest) = match line.as_bytes().first()? {
        b'+' => (b'+', &line[1..]),
        b'-' => (b'-', &line[1..]),
        _ => return None,
    };
    // rest starts with the device name; it ends at the first space (or " at ").
    let name = rest.split_whitespace().next()?;
    let digits = name.strip_prefix("hidraw")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let path = PathBuf::from(format!("/dev/{}", name));
    Some(if kind == b'+' { ParsedEvent::Attach(path) } else { ParsedEvent::Detach(path) })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ATTACH: &str = "+hidraw1 at index=255 page=0x0000 usage=0x0000 bus=0x03 vendor=0x1e54 product=0x2030 version=0x0150 on hidbus1";
    const DETACH: &str = "-hidraw1 at index=255 page=0x0000 usage=0x0000 bus=0x03 vendor=0x1e54 product=0x2030 version=0x0150 on hidbus1";

    #[test]
    fn parses_attach() {
        assert_eq!(parse_line(ATTACH), Some(ParsedEvent::Attach(PathBuf::from("/dev/hidraw1"))));
    }
    #[test]
    fn parses_detach() {
        assert_eq!(parse_line(DETACH), Some(ParsedEvent::Detach(PathBuf::from("/dev/hidraw1"))));
    }
    #[test]
    fn ignores_unrelated() {
        assert_eq!(parse_line("+hidbus1 at   on usbhid1"), None);
        assert_eq!(parse_line("+hkbd0 at index=0 page=0x0001 usage=0x0006"), None);
        assert_eq!(parse_line("!system=USB subsystem=DEVICE type=ATTACH ugen=ugen1.2"), None);
        assert_eq!(parse_line(""), None);
        assert_eq!(parse_line("? at index=2 page=0xff00"), None);
    }
    #[test]
    fn ignores_bad_hidraw_name() {
        assert_eq!(parse_line("+hidraw at   on hidbus1"), None);
        assert_eq!(parse_line("+hidrawX at   on hidbus1"), None);
    }
}
