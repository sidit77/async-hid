use crate::DeviceInfo;
use crate::error::{ErrorSource, HidResult};

pub async fn enumerate() -> HidResult<Vec<DeviceInfo>> {
    Ok(Vec::new())
}


pub type BackendError = windows::core::Error;

impl From<BackendError> for ErrorSource {
    fn from(value: BackendError) -> Self {
        ErrorSource::PlatformSpecific(value)
    }
}


/*
use std::time::Duration;
use windows::Devices::Enumeration::{DeviceInformation, DeviceInformationUpdate};
use windows::Foundation::TypedEventHandler;
use windows::h;

#[pollster::main]
async fn main() -> anyhow::Result<()> {
    println!("Start");
    let selector = h!(r#"System.Devices.InterfaceClassGuid:="{4D1E55B2-F16F-11CF-88CB-001111000030}" AND System.Devices.InterfaceEnabled:=System.StructuredQueryType.Boolean#True"#);

    let watcher = DeviceInformation::CreateWatcherAqsFilter(&selector)?;
    watcher.Added(&TypedEventHandler::new(|_, args: &Option<DeviceInformation>| {
        if let Some(args) = args {
            println!("Added: {}", args.Name()?);
        }
        Ok(())
    }))?;
    watcher.Updated(&TypedEventHandler::new(|_, args: &Option<DeviceInformationUpdate>| {
        if let Some(args) = args {
            println!("Updated: {}", args.Id()?);
        }
        Ok(())
    }))?;
    watcher.Removed(&TypedEventHandler::new(|_, args: &Option<DeviceInformationUpdate>| {
        if let Some(args) = args {
            println!("Removed: {}", args.Id()?);
        }
        Ok(())
    }))?;
    watcher.Start()?;
    std::thread::sleep(Duration::from_secs(100000000000));

    //let selector = h!(r#"System.Devices.InterfaceClassGuid:="{4526e8c1-8aac-4153-9b16-55e86ada0e54}""#);
    //let selector = HidDevice::GetDeviceSelectorVidPid(0xFFC0, 0x1, 0x1038, 0x2206)?;
    //let devices = DeviceInformation::FindAllAsyncAqsFilter(&selector)?.await?;
    //for device in devices {
    //    println!("Name: {}", device.Name()?);
//
    //    let device = HidDevice::FromIdAsync(&device.Id()?, FileAccessMode::Read)?;
    //    if let Ok(device) = device.await {
    //        println!("VenderId: 0x{:X}", device.VendorId()?);
    //        println!("ProductId: 0x{:X}", device.ProductId()?);
    //        println!("Version: 0x{:X}", device.Version()?);
    //        println!("UsagePage: 0x{:X}", device.UsagePage()?);
    //        println!("UsageId: 0x{:X}", device.UsageId()?);
    //    }
    //    println!();
    //}

    //if let Some(device) = devices.into_iter().next() {
    //    println!("Found device: {}", device.Name()?);
    //    let device = HidDevice::FromIdAsync(&device.Id()?, FileAccessMode::ReadWrite)?.await?;
    //    {
    //        let output = device.CreateOutputReport()?;
    //        let len = output.Data()?.Length()?;
    //        let writer = DataWriter::new()?;
    //        writer.WriteBytes(&[0x0, 0xb0])?;
    //        for _ in 0..(len - 2) {
    //            writer.WriteByte(0)?;
    //        }
    //        output.SetData(&writer.DetachBuffer()?)?;
    //        device.SendOutputReportAsync(&output)?.await?;
//
    //    }
    //    let (sender, receiver) = flume::unbounded();
    //    let token = device.InputReportReceived(&TypedEventHandler::new(move |_, args: &Option<HidInputReportReceivedEventArgs>| {
    //        if let Some(args) = args {
    //            let report = args.Report()?;
    //            sender.send(report).expect("fdg");
    //        }
    //        Ok(())
    //    }))?;
    //    while let Ok(report) = receiver.recv() {
    //        let buffer = report.Data()?;
    //        //if buffer.Length()? > 1 {
    //        let reader = DataReader::FromBuffer(&buffer)?;
    //        let mut bytes = [0u8; 8];
    //        reader.ReadBytes(&mut bytes)?;
    //        println!("{:?}", bytes);
    //        //}
    //    }
    //    device.RemoveInputReportReceived(token)?;
    //    //loop {
    //    //    let report = device.GetInputReportAsync()?.await?;
    //    //    let buffer = report.Data()?;
    //    //    if buffer.Length()? > 1 {
    //    //        let reader = DataReader::FromBuffer(&buffer)?;
    //    //        let mut bytes = vec![0u8; buffer.Length()? as usize];
    //    //        reader.ReadBytes(&mut bytes)?;
    //    //        println!("{:?}", bytes);
    //    //    }
    //    //}
    //}

    Ok(())
}
*/