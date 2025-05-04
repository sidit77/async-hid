use futures_lite::stream::Boxed;
use crate::backend::{Backend, DeviceInfoStream};
use crate::traits::{AsyncHidRead, AsyncHidWrite};
use crate::{DeviceEvent, DeviceId, HidResult};

#[derive(Default)]
pub struct IoHidManagerBackend2;

impl Backend for IoHidManagerBackend2 {
    type Reader = DummyRW;
    type Writer = DummyRW;

    async fn enumerate(&self) -> HidResult<DeviceInfoStream> {
        todo!()
    }

    fn watch(&self) -> HidResult<Boxed<DeviceEvent>> {
        todo!()
    }

    async fn open(&self, _id: &DeviceId, _read: bool, _write: bool) -> HidResult<(Option<Self::Reader>, Option<Self::Writer>)> {
        todo!()
    }
    
    
}

#[derive(Debug)]
pub struct DummyRW;

impl AsyncHidRead for DummyRW {
    async fn read_input_report<'a>(&'a mut self, _buf: &'a mut [u8]) -> HidResult<usize> {
        todo!()
    }
}

impl AsyncHidWrite for DummyRW {
    async fn write_output_report<'a>(&'a mut self, _buf: &'a [u8]) -> HidResult<()> {
        todo!()
    }
}