use super::SupportedConfigs;
use super::buffer;
use super::utils::{CpalResult, err};
use crate::ErrorKind::*;
use crate::*;
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::Duration;
use tap::Pipe;

#[derive(Debug)]
pub struct Session {
    stage: RwLock<Stage>,
}

impl Session {
    pub fn id(&self) -> DeviceId {
        todo!()
    }

    pub fn display_name(&self) -> String {
        todo!()
    }

    pub fn description(&self) -> CpalResult<DeviceDescription> {
        todo!()
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
