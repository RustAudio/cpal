use crate::traits::{DeviceTrait, HostTrait, StreamTrait};
use crate::{
    BuildStreamError, Data, DefaultStreamConfigError, DeviceNameError, DevicesError,
    InputCallbackInfo, OutputCallbackInfo, PauseStreamError, PlayStreamError, SampleFormat,
    StreamConfig, StreamError, SupportedStreamConfig, SupportedStreamConfigRange,
    SupportedStreamConfigsError,
};
use core::time::Duration;

pub(crate) type Devices = Box<dyn Iterator<Item = Box<dyn DeviceErased>>>;

pub(crate) trait HostErased {
    fn devices(&self) -> Result<Devices, DevicesError>;
    fn default_input_device(&self) -> Option<Box<dyn DeviceErased>>;
    fn default_output_device(&self) -> Option<Box<dyn DeviceErased>>;
}

pub(crate) type SupportedConfigs = Box<dyn Iterator<Item = SupportedStreamConfigRange>>;
pub(crate) type ErrorCallback = Box<dyn FnMut(StreamError) + Send + 'static>;
pub(crate) type InputCallback = Box<dyn FnMut(&Data, &InputCallbackInfo) + Send + 'static>;
pub(crate) type OutputCallback = Box<dyn FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static>;

pub(crate) trait DeviceErased {
    fn name(&self) -> Result<String, DeviceNameError>;
    fn supports_input(&self) -> bool;
    fn supports_output(&self) -> bool;
    fn supported_input_configs(&self) -> Result<SupportedConfigs, SupportedStreamConfigsError>;
    fn supported_output_configs(&self) -> Result<SupportedConfigs, SupportedStreamConfigsError>;
    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError>;
    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError>;
    fn build_input_stream_raw(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: InputCallback,
        error_callback: ErrorCallback,
        timeout: Option<Duration>,
    ) -> Result<Box<dyn StreamErased>, BuildStreamError>;
    fn build_output_stream_raw(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: OutputCallback,
        error_callback: ErrorCallback,
        timeout: Option<Duration>,
    ) -> Result<Box<dyn StreamErased>, BuildStreamError>;
}

pub(crate) trait StreamErased {
    fn play(&self) -> Result<(), PlayStreamError>;
    fn pause(&self) -> Result<(), PauseStreamError>;
}

fn device_to_erased(d: impl DeviceTrait + 'static) -> Box<dyn DeviceErased> {
    Box::new(d)
}

impl<T> HostErased for T
where
    T: HostTrait,
    T::Devices: 'static,
    T::Device: 'static,
{
    fn devices(&self) -> Result<Devices, DevicesError> {
        let iter = <T as HostTrait>::devices(self)?;
        let erased = Box::new(iter.map(device_to_erased));
        Ok(erased)
    }

    fn default_input_device(&self) -> Option<Box<dyn DeviceErased>> {
        <T as HostTrait>::default_input_device(self).map(device_to_erased)
    }

    fn default_output_device(&self) -> Option<Box<dyn DeviceErased>> {
        <T as HostTrait>::default_output_device(self).map(device_to_erased)
    }
}

fn supported_configs_to_erased(
    i: impl Iterator<Item = SupportedStreamConfigRange> + 'static,
) -> SupportedConfigs {
    Box::new(i)
}

fn stream_to_erased(s: impl StreamTrait + 'static) -> Box<dyn StreamErased> {
    Box::new(s)
}

impl<T> DeviceErased for T
where
    T: DeviceTrait,
    T::SupportedInputConfigs: 'static,
    T::SupportedOutputConfigs: 'static,
    T::Stream: 'static,
{
    fn name(&self) -> Result<String, DeviceNameError> {
        <T as DeviceTrait>::name(self)
    }

    fn supports_input(&self) -> bool {
        <T as DeviceTrait>::supports_input(self)
    }

    fn supports_output(&self) -> bool {
        <T as DeviceTrait>::supports_output(self)
    }

    fn supported_input_configs(&self) -> Result<SupportedConfigs, SupportedStreamConfigsError> {
        <T as DeviceTrait>::supported_input_configs(self).map(supported_configs_to_erased)
    }

    fn supported_output_configs(&self) -> Result<SupportedConfigs, SupportedStreamConfigsError> {
        <T as DeviceTrait>::supported_input_configs(self).map(supported_configs_to_erased)
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        <T as DeviceTrait>::default_input_config(self)
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        <T as DeviceTrait>::default_output_config(self)
    }

    fn build_input_stream_raw(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: InputCallback,
        error_callback: ErrorCallback,
        timeout: Option<Duration>,
    ) -> Result<Box<dyn StreamErased>, BuildStreamError> {
        <T as DeviceTrait>::build_input_stream_raw(
            self,
            config,
            sample_format,
            data_callback,
            error_callback,
            timeout,
        )
        .map(stream_to_erased)
    }

    fn build_output_stream_raw(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: OutputCallback,
        error_callback: ErrorCallback,
        timeout: Option<Duration>,
    ) -> Result<Box<dyn StreamErased>, BuildStreamError> {
        <T as DeviceTrait>::build_output_stream_raw(
            self,
            config,
            sample_format,
            data_callback,
            error_callback,
            timeout,
        )
        .map(stream_to_erased)
    }
}

impl<T> StreamErased for T
where
    T: StreamTrait,
{
    fn play(&self) -> Result<(), PlayStreamError> {
        <T as StreamTrait>::play(self)
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        <T as StreamTrait>::pause(self)
    }
}
