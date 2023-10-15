use std::pin::Pin;
use std::task::{Context, Poll};
use futures_core::Stream;
use serde::Serialize;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Hid, HidDeviceRequestOptions, window};

use crate::{DeviceInfo, AccessMode, ensure, HidError};
use crate::error::HidResult;

fn webhid() -> HidResult<Hid> {
    let window = window()
        .ok_or(HidError::custom("Failed to get window"))?;
    let hid = window.navigator().hid();
    ensure!(!hid.is_undefined(), HidError::custom("WebHid is not supported on this browser"));
    Ok(hid)
}

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceFilter {
    vendor_id: Option<u16>,
    product_id: Option<u16>,
    usage_page: Option<u16>,
    usage: Option<u16>
}

impl DeviceFilter {

    pub const fn new() -> Self {
        Self {
            vendor_id: None,
            product_id: None,
            usage_page: None,
            usage: None,
        }
    }

    pub const fn with_vendor_id(mut self, id: u16) -> Self {
        self.vendor_id = Some(id);
        self
    }

    pub const fn with_product_id(mut self, id: u16) -> Self {
        self.product_id = Some(id);
        self
    }

    pub const fn with_usage_page(mut self, id: u16) -> Self {
        self.usage_page = Some(id);
        self
    }

    pub const fn with_usage(mut self, id: u16) -> Self {
        self.usage = Some(id);
        self
    }

}

pub async fn enumerate() -> HidResult<impl Stream<Item = DeviceInfo> + Send + Unpin> {
    const FILTERS: &[DeviceFilter] = &[
        DeviceFilter::new()
            .with_vendor_id(0x1038)
    ];

    let hid = webhid()?;
    let filters = serde_wasm_bindgen::to_value(FILTERS)
        .expect("Failed to serialize filter");
    log::info!("{:?}", filters);
    let request = hid.request_device(&HidDeviceRequestOptions::new(&filters));
    log::info!("future");
    let request = JsFuture::from(request).await.unwrap();
    log::info!("{:?}", request);

    Ok(DummyStream)
}

struct DummyStream;
impl Stream for DummyStream {
    type Item = DeviceInfo;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(None)
    }
}

pub async fn open(_id: &BackendDeviceId, _mode: AccessMode) -> HidResult<BackendDevice> {
    todo!()
}

#[derive(Debug, Clone)]
pub struct BackendDevice {}

impl BackendDevice {
    pub async fn read_input_report(&self, _buf: &mut [u8]) -> HidResult<usize> {
        todo!()
    }

    pub async fn write_output_report(&self, _data: &[u8]) -> HidResult<()> {
        todo!()
    }
}

pub type BackendDeviceId = u32;

pub type BackendError = ();

pub type BackendPrivateData = ();
