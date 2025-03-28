use std::num::NonZeroU8;
use std::ops::{Deref, DerefMut};

#[derive(Clone)]
#[repr(transparent)]
pub struct Report(pub(crate) Vec<u8>);

impl Report {
    
    pub fn new(report_id: Option<NonZeroU8>, size: usize) -> Self {
        let mut new = Self(vec![0; size + 1]);
        new.set_report_id(report_id);
        new
    }
    
    pub fn from_bytes(report_id: Option<NonZeroU8>, bytes: &[u8]) -> Self {
        let mut new = Self::new(report_id, bytes.len());
        new.copy_from_slice(bytes);
        new
    }
    
    pub fn set_report_id(&mut self, report_id: Option<NonZeroU8>) {
        // SAFETY Option<NonZeroU8> is guaranteed to be compatible with u8
        self.0[0] = unsafe { std::mem::transmute(report_id) };
    }
    
    pub fn get_report_id(&self) -> Option<NonZeroU8> {
        // SAFETY Option<NonZeroU8> is guaranteed to be compatible with u8
        unsafe { std::mem::transmute(self.0[0]) }
    }
    
    pub fn len(&self) -> usize {
        self.0.len()
    }
    
}

impl Deref for Report {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0[1..]
    }
}

impl DerefMut for Report {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0[1..]
    }
}
