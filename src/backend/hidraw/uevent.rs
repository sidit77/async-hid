use std::path::Path;

pub enum Action<'a> {
    Add,
    Remove,
    Other(&'a str)
}

pub struct UEvent<'a> {
    pub action: Action<'a>,
    pub subsystem: &'a str,
    pub dev_path: &'a Path
}

impl<'a> UEvent<'a> {
    pub fn parse(event: &'a [u8]) -> Result<UEvent<'a>, &'static str> {
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
