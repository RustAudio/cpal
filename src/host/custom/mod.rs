use crate::traits::{DeviceTrait, HostTrait, StreamTrait};
use crate::{
    BuildStreamError, Data, DefaultStreamConfigError, DeviceNameError, DevicesError,
    InputCallbackInfo, OutputCallbackInfo, PauseStreamError, PlayStreamError, SampleFormat,
    StreamConfig, StreamError, SupportedStreamConfig, SupportedStreamConfigRange,
    SupportedStreamConfigsError,
};
use core::time::Duration;

pub struct Host(Box<dyn HostErased>);

impl Host {
    pub(crate) fn new() -> Result<Self, crate::HostUnavailable> {
        Err(crate::HostUnavailable)
    }

    pub fn from_host<T>(host: T) -> Self
    where
        T: HostTrait + 'static,
        T::Device: Clone,
        <T::Device as DeviceTrait>::SupportedInputConfigs: Clone,
        <T::Device as DeviceTrait>::SupportedOutputConfigs: Clone,
    {
        Self(Box::new(host))
    }
}

pub struct Device(Box<dyn DeviceErased>);

impl Clone for Device {
    fn clone(&self) -> Self {
        self.0.clone()
    }
}

pub struct Stream(Box<dyn StreamErased>);

// -----

type Devices = Box<dyn Iterator<Item = Device>>;
trait HostErased {
    fn devices(&self) -> Result<Devices, DevicesError>;
    fn default_input_device(&self) -> Option<Device>;
    fn default_output_device(&self) -> Option<Device>;
}

pub struct SupportedConfigs(Box<dyn SupportedConfigsErased>);

trait SupportedConfigsErased {
    fn next(&mut self) -> Option<SupportedStreamConfigRange>;

    fn clone(&self) -> SupportedConfigs;
}

impl<T> SupportedConfigsErased for T
where
    T: Iterator<Item = SupportedStreamConfigRange> + Clone + 'static,
{
    fn next(&mut self) -> Option<SupportedStreamConfigRange> {
        <Self as Iterator>::next(self)
    }

    fn clone(&self) -> SupportedConfigs {
        SupportedConfigs(Box::new(Clone::clone(self)))
    }
}

impl Iterator for SupportedConfigs {
    type Item = SupportedStreamConfigRange;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

impl Clone for SupportedConfigs {
    fn clone(&self) -> Self {
        self.0.clone()
    }
}

type ErrorCallback = Box<dyn FnMut(StreamError) + Send + 'static>;
type InputCallback = Box<dyn FnMut(&Data, &InputCallbackInfo) + Send + 'static>;
type OutputCallback = Box<dyn FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static>;

trait DeviceErased {
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
    ) -> Result<Stream, BuildStreamError>;
    fn build_output_stream_raw(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: OutputCallback,
        error_callback: ErrorCallback,
        timeout: Option<Duration>,
    ) -> Result<Stream, BuildStreamError>;

    fn clone(&self) -> Device;
}

trait StreamErased {
    fn play(&self) -> Result<(), PlayStreamError>;
    fn pause(&self) -> Result<(), PauseStreamError>;
}

fn device_to_erased(d: impl DeviceErased + 'static) -> Device {
    Device(Box::new(d))
}

impl<T> HostErased for T
where
    T: HostTrait,
    T::Devices: 'static,
    T::Device: DeviceErased + 'static,
{
    fn devices(&self) -> Result<Devices, DevicesError> {
        let iter = <T as HostTrait>::devices(self)?;
        let erased = Box::new(iter.map(device_to_erased));
        Ok(erased)
    }

    fn default_input_device(&self) -> Option<Device> {
        <T as HostTrait>::default_input_device(self).map(device_to_erased)
    }

    fn default_output_device(&self) -> Option<Device> {
        <T as HostTrait>::default_output_device(self).map(device_to_erased)
    }
}

fn supported_configs_to_erased(
    i: impl Iterator<Item = SupportedStreamConfigRange> + Clone + 'static,
) -> SupportedConfigs {
    SupportedConfigs(Box::new(i))
}

fn stream_to_erased(s: impl StreamTrait + 'static) -> Stream {
    Stream(Box::new(s))
}

impl<T> DeviceErased for T
where
    T: DeviceTrait + Clone + 'static,
    T::SupportedInputConfigs: Clone + 'static,
    T::SupportedOutputConfigs: Clone + 'static,
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
    ) -> Result<Stream, BuildStreamError> {
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
    ) -> Result<Stream, BuildStreamError> {
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

    fn clone(&self) -> Device {
        device_to_erased(Clone::clone(self))
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

// -----

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        false
    }

    fn devices(&self) -> Result<Self::Devices, DevicesError> {
        self.0.devices()
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        self.0.default_input_device()
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        self.0.default_output_device()
    }
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedConfigs;

    type SupportedOutputConfigs = SupportedConfigs;

    type Stream = Stream;

    fn name(&self) -> Result<String, DeviceNameError> {
        self.0.name()
    }

    fn supports_input(&self) -> bool {
        self.0.supports_input()
    }

    fn supports_output(&self) -> bool {
        self.0.supports_output()
    }

    fn supported_input_configs(
        &self,
    ) -> Result<Self::SupportedInputConfigs, SupportedStreamConfigsError> {
        self.0.supported_input_configs()
    }

    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, SupportedStreamConfigsError> {
        self.0.supported_output_configs()
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        self.0.default_input_config()
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        self.0.default_output_config()
    }

    fn build_input_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        self.0.build_input_stream_raw(
            config,
            sample_format,
            Box::new(data_callback),
            Box::new(error_callback),
            timeout,
        )
    }

    fn build_output_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        self.0.build_output_stream_raw(
            config,
            sample_format,
            Box::new(data_callback),
            Box::new(error_callback),
            timeout,
        )
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        self.0.play()
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        self.0.pause()
    }
}
