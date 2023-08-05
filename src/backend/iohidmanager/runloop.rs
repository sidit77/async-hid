use std::ffi::c_void;
use std::fmt::{Debug, Formatter};
use std::ptr::null_mut;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;
use core_foundation::base::{kCFAllocatorDefault, TCFType};
use core_foundation::runloop::{CFRunLoop, CFRunLoopRunInMode, CFRunLoopRunResult, CFRunLoopSource, CFRunLoopSourceContext, CFRunLoopSourceCreate, CFRunLoopSourceSignal, CFRunLoopStop, CFRunLoopWakeUp};
use core_foundation::string::CFString;
use tokio::sync::mpsc::{Sender, unbounded_channel, UnboundedSender};
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::oneshot::channel;
use crate::backend::iohidmanager::device::IOHIDDevice;
use crate::{HidError, HidResult};
use crate::backend::open;

struct LoopSource(CFRunLoopSource, CFRunLoop);
unsafe impl Send for LoopSource {}

impl Debug for LoopSource {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("LoopSource").finish()
    }
}

impl LoopSource {
    fn signal(&self) {
        unsafe {
            CFRunLoopSourceSignal(self.0.as_concrete_TypeRef());
            CFRunLoopWakeUp(self.1.as_concrete_TypeRef());
        };
    }
}

struct LoopSender<T> {
    source: LoopSource,
    sender: UnboundedSender<T>
}

impl<T> LoopSender<T> {

    fn send(&self, item: T) -> HidResult<()> {
        self.sender.send(item)
            .map_err(|_| HidError::custom("Loop is closed"))?;
        self.source.signal();
        Ok(())
    }

}

//struct RunLoopState {
//    mode: CFString,
//    run_loop: CFRunLoop
//}

#[derive(Debug)]
enum LoopCommand {
    Stop,
    Schedule(IOHIDDevice),
    Unschedule(IOHIDDevice)
}

pub struct RunLoop {
    sender: LoopSender<LoopCommand>,
    thread: Option<JoinHandle<()>>
}

impl RunLoop {

    pub async fn new() -> HidResult<Self> {
        let (sender, receiver) = channel::<LoopSender<LoopCommand>>();

        let thread = Some(thread::spawn(|| {
            let run_loop_mode = CFString::new(&format!("ASYNC_HID_{:?}", thread::current().id()));
            let run_loop = CFRunLoop::get_current();

            extern "C" fn dummy(_: *const c_void) {}
            let mut ctx = CFRunLoopSourceContext {
                version: 0,
                info: null_mut(),
                retain: None,
                release: None,
                copyDescription: None,
                equal: None,
                hash: None,
                schedule: None,
                cancel: None,
                perform: dummy,
            };

            let source = unsafe {
                let s = CFRunLoopSourceCreate(kCFAllocatorDefault, 0 /* order */, &mut ctx);
                CFRunLoopSource::wrap_under_create_rule(s)
            };
            run_loop.add_source(&source, run_loop_mode.as_concrete_TypeRef());

            let (ext_sender, mut receiver) = unbounded_channel();
            let ext_sender = LoopSender {
                source: LoopSource(source, CFRunLoop::get_current()),
                sender: ext_sender,
            };
            sender.send(ext_sender).unwrap_or_else(|_| panic!("Failed to send sender"));

            'outer: loop {
                let result = CFRunLoop::run_in_mode(run_loop_mode.as_concrete_TypeRef(), Duration::from_secs(1000), true);
                match result {
                    CFRunLoopRunResult::TimedOut => continue,
                    CFRunLoopRunResult::HandledSource => loop {
                        match receiver.try_recv() {
                            Ok(cmd) => match cmd {
                                LoopCommand::Stop => {
                                    run_loop.stop();
                                    break 'outer;
                                },
                                LoopCommand::Schedule(dev) => dev.schedule_with_runloop(&run_loop, &run_loop_mode),
                                LoopCommand::Unschedule(dev) => dev.unschedule_from_runloop(&run_loop, &run_loop_mode),
                            },
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => break 'outer
                        }
                    },
                    _ => break
                }
            }
        }));

        let sender = receiver
            .await
            .map_err(|_| HidError::custom("Run loop failed to start"))?;

        Ok(Self {
            sender,
            thread,
        })
    }

    pub fn schedule_device(&self, device: &IOHIDDevice) -> HidResult<()> {
        self.sender.send(LoopCommand::Schedule(device.clone()))?;
        Ok(())
    }

    pub fn unschedule_device(&self, device: &IOHIDDevice) -> HidResult<()> {
        self.sender.send(LoopCommand::Unschedule(device.clone()))?;
        Ok(())
    }

}

impl Drop for RunLoop {
    fn drop(&mut self) {
        self.sender.send(LoopCommand::Stop).unwrap();
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

//static CURRENT_RUN_LOOP: Mutex<Option<Weak<RunLoop>>> = Mutex::const_new(None);
//
//pub async fn get_run_loop() -> HidResult<Arc<RunLoop>> {
//    let mut lock = CURRENT_RUN_LOOP.lock().await;
//    let current = lock
//        .take()
//        .and_then(|weak| weak.upgrade());
//    let current = match current {
//        None => Arc::new(RunLoop::new().await?),
//        Some(current) => current
//    };
//    *lock = Some(Arc::downgrade(&current));
//    Ok(current)
//}