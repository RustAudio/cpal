use super::SupportedConfigs;
use super::buffer;
use super::utils::{CpalResult, err};
use crate::ErrorKind::*;
use crate::*;
use azo::WinResult;
use azo::driver::{Driver, Proxy};
use azo::dto::ChannelCounts;
use azo::utils::Host as AzoHost;
use azo::windows_core::GUID;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use std::time::Duration;
use tap::Pipe;

/// Keeps track of all created [`Session`]s to prevent creating
/// multiple instances of the same driver in the same `AzoHost`
#[derive(Debug)]
pub struct Factory {
    azo_host: Arc<AzoHost>,
    cache: Mutex<HashMap<GUID, Weak<Session>>>,
}

impl Factory {
    pub fn new() -> Self {
        Self {
            azo_host: AzoHost::new(),
            cache: Mutex::default(),
        }
    }

    pub fn get_session(&self, clsid: &GUID) -> WinResult<Arc<Session>> {
        let mut guard = self.cache.lock();

        if let Some(existing) = guard.get(clsid).and_then(Weak::upgrade) {
            return Ok(existing);
        }

        let driver = self
            .azo_host
            .create_driver(*clsid)?
            .pipe(Handle);

        let new = Session
            ::new(driver, clsid)?
            .pipe(Arc::new);

        guard.insert(*clsid, Arc::downgrade(&new));

        Ok(new)
    }
}

/// A "cpal-ified" driver handle where relevant types (such as errors) are mapped to cpal's equivalents
#[derive(Debug)]
struct Handle(Proxy);

impl Handle {
    pub fn name(&self) -> String {
        self.0
            .name()
            .to_string_lossy()
            .into_owned()
    }

    pub fn channel_counts(&self) -> CpalResult<ChannelCounts> {
        self.0
            .channel_counts()
            .map_err(|error| Error::with_message(BackendError, format!("failed to retrieve channel coounts: {error}")))
    }
}

#[derive(Debug)]
pub struct Session {
    driver: Handle,
    stage: RwLock<Stage>,
    clsid_string: String,
}

impl Session {
	fn new(driver: Handle, clsid: &GUID) -> WinResult<Self> {
        let stage =
            if driver.0.init(None) { Stage::Initialized }
            else                   { Stage::Loaded };

        Self {
            driver,
            stage: stage.into(),
            clsid_string: format!("{clsid:?}"),
        }
        .pipe(Ok)
    }

    pub fn id(&self) -> DeviceId {
        DeviceId::new(HostId::AsioNew, &self.clsid_string)
    }

    pub fn display_name(&self) -> String {
        self.driver.name()
    }

    pub fn description(&self) -> CpalResult<DeviceDescription> {
        let stage = self.stage.read();

        let name = self.driver.name();
        let direction = self.driver.channel_counts()?.into();
        let mut extended = vec![format!("driver version: {}", self.driver.0.version())];

        if matches!(*stage, Stage::Loaded) {
            extended.push("ASIO driver failed to initialize".to_owned());
            extended.push(format!("last error: {}", self.driver.0.last_error().to_string_lossy()));
        }

        DeviceDescriptionBuilder
            ::new(&name)
            .driver(name)
            .direction(direction)
            .extended(extended)
            .build()
            .pipe(Ok)
    }

    #[must_use]
    pub fn supports_direction<const IN: bool, const OUT: bool>(&self) -> bool {
        todo!()
    }

    pub fn supported_configs<const INPUT: bool>(&self) -> CpalResult<SupportedConfigs> {
        todo!()
    }

    pub fn default_config<const INPUT: bool>(&self) -> CpalResult<SupportedStreamConfig> {
        todo!()
    }

    pub fn build_stream(
        self       : &Arc<Self>,
        cfg_in     : buffer::Config,
        cfg_out    : buffer::Config,
        sample_rate: SampleRate,
        buffer_size: BufferSize,
        data_cb    : data_cb_type!(),
        error_cb   : error_cb_type!(),
    ) -> CpalResult<super::Stream> {
        let mut stage = self.stage.write();

        match *stage {
            Stage::Loaded          => return err(DeviceNotAvailable, "ASIO driver failed to initialize"),
            Stage::Initialized     => (), // this is the appropiate stage for calling this function
            Stage::Prepared { .. } => return err(UnsupportedOperation, "ASIO only supports 1 stream per device, and there already exists a stream for this device"),
        }

        todo!();

        *stage = Stage::Prepared {
            running: false
        };

        super::Stream {
            session: Arc::clone(self),
        }
        .pipe(Ok)
    }

    pub fn start(&self) -> CpalResult<()> {
        let mut stage = self.stage.write();

        if *stage.running()? {
            return Ok(());
        }

        todo!();

        *stage.running()? = true;
        Ok(())
    }

    pub fn pause(&self) -> CpalResult<()> {
        let mut stage = self.stage.write();

        if !*stage.running()? {
            return Ok(());
        }

        todo!();

        *stage.running()? = false;
        Ok(())
    }

    pub fn stop(&self, max_wait: Option<Duration>) -> CpalResult<()> {
        let mut stage = self.stage.write();

        if !*stage.running()? {
            return Ok(());
        }

        todo!();

        *stage.running()? = false;
        Ok(())
    }

    pub fn now(&self) -> StreamInstant {
        todo!()
    }

    pub fn reset(&self) -> CpalResult<()> {
        let mut stage = self.stage.write();

        if *stage.running()? {
            todo!();
        }

        todo!();

        *stage = Stage::Initialized;
        Ok(())
    }
}

/// ASIO lifecycle stage (see ASIO specification section II.2)
#[derive(Debug)]
pub enum Stage {
    Loaded,
    Initialized,
    Prepared {
        running: bool,
    },
}

impl Stage {
    fn running(&mut self) -> CpalResult<&mut bool> {
        match self {
            Self::Prepared { running } => Ok(running),
            _ => err(Other, "BUG! This branch should be unreachable"),
        }
    }
}
