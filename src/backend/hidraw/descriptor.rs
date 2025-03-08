use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use crate::HidResult;

#[derive(Default)]
pub struct HidrawReportDescriptor(Vec<u8>);

impl HidrawReportDescriptor {
    /// Open and parse given the "base" sysfs of the device
    pub fn from_syspath(syspath: &Path) -> HidResult<Self> {
        let path = syspath.join("device/report_descriptor");
        let mut f = File::open(path)?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;

        Ok(HidrawReportDescriptor(buf))
    }

    /// Create a descriptor from a slice
    ///
    /// It returns an error if the value slice is too large for it to be a HID
    /// descriptor
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn from_slice(value: &[u8]) -> HidResult<Self> {
        Ok(HidrawReportDescriptor(value.to_vec()))
    }

    pub fn usages(&self) -> impl Iterator<Item = (u16, u16)> + '_ {
        UsageIterator {
            usage_page: 0,
            cursor: Cursor::new(&self.0)
        }
    }
}

/// Iterates over the values in a HidrawReportDescriptor
struct UsageIterator<'a> {
    usage_page: u16,
    cursor: Cursor<&'a Vec<u8>>
}

impl Iterator for UsageIterator<'_> {
    type Item = (u16, u16);

    fn next(&mut self) -> Option<Self::Item> {
        let (usage_page, page) = next_hid_usage(&mut self.cursor, self.usage_page)?;

        self.usage_page = usage_page;
        Some((usage_page, page))
    }
}

// This comes from hidapi which apparently comes from Apple's implementation of
// this
fn next_hid_usage(cursor: &mut Cursor<&Vec<u8>>, mut usage_page: u16) -> Option<(u16, u16)> {
    let mut usage = None;
    let mut usage_pair = None;
    let initial = cursor.position() == 0;

    while let Some(Ok(key)) = cursor.bytes().next() {
        // The amount to skip is calculated based off of the start of the
        // iteration so we need to keep track of that.
        let position = cursor.position() - 1;
        let key_cmd = key & 0xfc;

        let (data_len, key_size) = hid_item_size(key, cursor)?;

        match key_cmd {
            // Usage Page 6.2.2.7 (Global)
            0x4 => {
                usage_page = match hid_report_bytes(cursor, data_len) {
                    Ok(v) => v as u16,
                    Err(_) => break,
                }
            }
            // Usage 6.2.2.8 (Local)
            0x8 => {
                usage = match hid_report_bytes(cursor, data_len) {
                    Ok(v) => Some(v as u16),
                    Err(_) => break,
                }
            }
            // Collection 6.2.2.4 (Main)
            0xa0 => {
                // Usage is a Local Item, unset it
                if let Some(u) = usage.take() {
                    usage_pair = Some((usage_page, u))
                }
            }
            // Input 6.2.2.4 (Main)
            0x80 |
            // Output 6.2.2.4 (Main)
            0x90 |
            // Feature 6.2.2.4 (Main)
            0xb0 |
            // End Collection 6.2.2.4 (Main)
            0xc0  =>  {
                // Usage is a Local Item, unset it
                usage.take();
            }
            _ => {}
        }

        if cursor
            .seek(SeekFrom::Start(position + (data_len + key_size) as u64))
            .is_err()
        {
            return None;
        }

        if let Some((usage_page, usage)) = usage_pair {
            return Some((usage_page, usage));
        }
    }

    if let (true, Some(usage)) = (initial, usage) {
        return Some((usage_page, usage));
    }

    None
}

/// Gets the size of the HID item at the given position
///
/// Returns data_len and key_size when successful
fn hid_item_size(key: u8, cursor: &mut Cursor<&Vec<u8>>) -> Option<(usize, usize)> {
    // Long Item. Next byte contains the length of the data section.
    if (key & 0xf0) == 0xf0 {
        if let Some(Ok(len)) = cursor.bytes().next() {
            return Some((len.into(), 3));
        }

        // Malformed report
        return None;
    }

    // Short Item. Bottom two bits contains the size code
    match key & 0x03 {
        v @ 0..=2 => Some((v.into(), 1)),
        3 => Some((4, 1)),
        _ => unreachable!() // & 0x03 means this can't happen
    }
}

/// Get the bytes from a HID report descriptor
///
/// Must only be called with `num_bytes` 0, 1, 2 or 4.
fn hid_report_bytes(cursor: &mut Cursor<&Vec<u8>>, num_bytes: usize) -> HidResult<u32> {
    let mut bytes: [u8; 4] = [0; 4];
    cursor.read_exact(&mut bytes[..num_bytes])?;

    Ok(u32::from_le_bytes(bytes))
}
