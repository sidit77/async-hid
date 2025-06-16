use std::path::Path;
use crate::ensure;

#[derive(Debug)]
pub enum Action<'a> {
    Add,
    Remove,
    Other(&'a str)
}

#[derive(Debug)]
pub struct UEvent<'a> {
    pub action: Action<'a>,
    pub subsystem: &'a str,
    pub dev_path: &'a Path
}

impl<'a> UEvent<'a> {
    pub fn parse(event: &'a [u8]) -> Result<UEvent<'a>, &'static str> {
        ensure!(event.len() >= 32, "Message is too short");
        let offset = if event[0..8].eq(b"libudev\0") {
            const UDEV_MONITOR_MAGIC: u32 = 0xfeedcafe;
            let magic = u32::from_be_bytes(event[8..12].try_into().unwrap());
            ensure!(magic == UDEV_MONITOR_MAGIC, "Invalid magic number");
            
            u32::from_ne_bytes(event[16..20].try_into().unwrap()) as usize
        } else {
            ensure!(event.contains(&b'@'), "Invalid kernel event");
            event.iter().position(|&b| b == b'\0').ok_or("Failed to find the start of the message")? + 1
        };
        
        Self::parse_internal(&event[offset..])
    }
    fn parse_internal(event: &'a [u8]) -> Result<UEvent<'a>, &'static str> {
        let mut action = None;
        let mut subsystem = None;
        let mut dev_path = None;
        
        for line in std::str::from_utf8(event)
            .map_err(|_| "Invalid utf-8")?
            .split('\0')
        {
            if let Some((key, value)) = line.split_once('=') {
                match key {
                    "ACTION" => {
                        action = Some(match value {
                            "add" => Action::Add,
                            "remove" => Action::Remove,
                            other => Action::Other(other)
                        })
                    }
                    "SUBSYSTEM" => subsystem = Some(value),
                    "DEVPATH" => dev_path = Some(Path::new(value)),
                    _ => {}
                }
            }
        }

        Ok(UEvent {
            action: action.ok_or("Event action not found")?,
            subsystem: subsystem.ok_or("Event subsystem not found")?,
            dev_path: dev_path.ok_or("Event device path not found")?
        })
    }
}
