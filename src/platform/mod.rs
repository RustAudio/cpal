//! Platform-specific items.
//!
//! This module also contains the implementation of the platform's dynamically dispatched `Host`
//! type and its associated `EventLoop`, `Device`, `StreamId` and other associated types. These
//! types are useful in the case that users require switching between audio host APIs at runtime.

#[doc(inline)]
pub use self::platform_impl::*;

// A macro to assist with implementing a platform's dynamically dispatched `Host` type.
//
// These dynamically dispatched types are necessary to allow for users to switch between hosts at
// runtime.
//
// For example the invocation `impl_platform_host(Wasapi wasapi "WASAPI", Asio asio "ASIO")`,
// this macro should expand to:
//
// ```
// pub enum HostId {
//     Wasapi,
//     Asio,
// }
//
// pub enum Host {
//     Wasapi(crate::host::wasapi::Host),
//     Asio(crate::host::asio::Host),
// }
// ```
//
// And so on for Device, Devices, EventLoop, Host, StreamId, SupportedInputFormats,
// SupportedOutputFormats and all their necessary trait implementations.
// ```
macro_rules! impl_platform_host {
    ($($HostVariant:ident $host_mod:ident $host_name:literal),*) => {
        /// All hosts supported by CPAL on this platform.
        pub const ALL_HOSTS: &'static [HostId] = &[
            $(
                HostId::$HostVariant,
            )*
        ];

        /// The platform's dynamically dispatched **Host** type.
        ///
        /// An instance of this **Host** type may represent one of any of the **Host**s available
        /// on the platform.
        ///
        /// Use this type if you require switching between available hosts at runtime.
        ///
        /// This type may be constructed via the **host_from_id** function. **HostId**s may
        /// be acquired via the **ALL_HOSTS** const and the **available_hosts** function.
        pub struct Host(HostInner);

        /// The **Device** implementation associated with the platform's dynamically dispatched
        /// **Host** type.
        pub struct Device(DeviceInner);

        /// The **Devices** iterator associated with the platform's dynamically dispatched **Host**
        /// type.
        pub struct Devices(DevicesInner);

        /// The **Stream** implementation associated with the platform's dynamically dispatched
        /// **Host** type.
        pub struct Stream(StreamInner);

        /// The **SupportedInputFormats** iterator associated with the platform's dynamically
        /// dispatched **Host** type.
        pub struct SupportedInputFormats(SupportedInputFormatsInner);

        /// The **SupportedOutputFormats** iterator associated with the platform's dynamically
        /// dispatched **Host** type.
        pub struct SupportedOutputFormats(SupportedOutputFormatsInner);

        /// Unique identifier for available hosts on the platform.
        #[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
        pub enum HostId {
            $(
                $HostVariant,
            )*
        }

        enum DeviceInner {
            $(
                $HostVariant(crate::host::$host_mod::Device),
            )*
        }

        enum DevicesInner {
            $(
                $HostVariant(crate::host::$host_mod::Devices),
            )*
        }

        enum HostInner {
            $(
                $HostVariant(crate::host::$host_mod::Host),
            )*
        }

        enum StreamInner {
            $(
                $HostVariant(crate::host::$host_mod::Stream),
            )*
        }

        enum SupportedInputFormatsInner {
            $(
                $HostVariant(crate::host::$host_mod::SupportedInputFormats),
            )*
        }

        enum SupportedOutputFormatsInner {
            $(
                $HostVariant(crate::host::$host_mod::SupportedOutputFormats),
            )*
        }

        impl HostId {
            pub fn name(&self) -> &'static str {
                match self {
                    $(
                        HostId::$HostVariant => $host_name,
                    )*
                }
            }
        }

        impl Host {
            /// The unique identifier associated with this host.
            pub fn id(&self) -> HostId {
                match self.0 {
                    $(
                        HostInner::$HostVariant(_) => HostId::$HostVariant,
                    )*
                }
            }
        }

        impl Iterator for Devices {
            type Item = Device;

            fn next(&mut self) -> Option<Self::Item> {
                match self.0 {
                    $(
                        DevicesInner::$HostVariant(ref mut d) => {
                            d.next().map(DeviceInner::$HostVariant).map(Device)
                        }
                    )*
                }
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                match self.0 {
                    $(
                        DevicesInner::$HostVariant(ref d) => d.size_hint(),
                    )*
                }
            }
        }

        impl Iterator for SupportedInputFormats {
            type Item = crate::SupportedFormat;

            fn next(&mut self) -> Option<Self::Item> {
                match self.0 {
                    $(
                        SupportedInputFormatsInner::$HostVariant(ref mut s) => s.next(),
                    )*
                }
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                match self.0 {
                    $(
                        SupportedInputFormatsInner::$HostVariant(ref d) => d.size_hint(),
                    )*
                }
            }
        }

        impl Iterator for SupportedOutputFormats {
            type Item = crate::SupportedFormat;

            fn next(&mut self) -> Option<Self::Item> {
                match self.0 {
                    $(
                        SupportedOutputFormatsInner::$HostVariant(ref mut s) => s.next(),
                    )*
                }
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                match self.0 {
                    $(
                        SupportedOutputFormatsInner::$HostVariant(ref d) => d.size_hint(),
                    )*
                }
            }
        }

        impl crate::traits::DeviceTrait for Device {
            type SupportedInputFormats = SupportedInputFormats;
            type SupportedOutputFormats = SupportedOutputFormats;
            type Stream = Stream;

            fn name(&self) -> Result<String, crate::DeviceNameError> {
                match self.0 {
                    $(
                        DeviceInner::$HostVariant(ref d) => d.name(),
                    )*
                }
            }

            fn supported_input_formats(&self) -> Result<Self::SupportedInputFormats, crate::SupportedFormatsError> {
                match self.0 {
                    $(
                        DeviceInner::$HostVariant(ref d) => {
                            d.supported_input_formats()
                                .map(SupportedInputFormatsInner::$HostVariant)
                                .map(SupportedInputFormats)
                        }
                    )*
                }
            }

            fn supported_output_formats(&self) -> Result<Self::SupportedOutputFormats, crate::SupportedFormatsError> {
                match self.0 {
                    $(
                        DeviceInner::$HostVariant(ref d) => {
                            d.supported_output_formats()
                                .map(SupportedOutputFormatsInner::$HostVariant)
                                .map(SupportedOutputFormats)
                        }
                    )*
                }
            }

            fn default_input_format(&self) -> Result<crate::Format, crate::DefaultFormatError> {
                match self.0 {
                    $(
                        DeviceInner::$HostVariant(ref d) => d.default_input_format(),
                    )*
                }
            }

            fn default_output_format(&self) -> Result<crate::Format, crate::DefaultFormatError> {
                match self.0 {
                    $(
                        DeviceInner::$HostVariant(ref d) => d.default_output_format(),
                    )*
                }
            }

            fn build_input_stream<D, E>(&self, format: &crate::Format, data_callback: D, error_callback: E) -> Result<Self::Stream, crate::BuildStreamError>
                where D: FnMut(crate::StreamData) + Send + 'static, E: FnMut(crate::StreamError) + Send + 'static {
                match self.0 {
                    $(
                        DeviceInner::$HostVariant(ref d) => d.build_input_stream(format, data_callback, error_callback)
                            .map(StreamInner::$HostVariant)
                            .map(Stream),
                    )*
                }
            }

            fn build_output_stream<D, E>(&self, format: &crate::Format, data_callback: D, error_callback: E) -> Result<Self::Stream, crate::BuildStreamError>
                where D: FnMut(crate::StreamData) + Send + 'static, E: FnMut(crate::StreamError) + Send + 'static {
                match self.0 {
                    $(
                        DeviceInner::$HostVariant(ref d) => d.build_output_stream(format, data_callback, error_callback)
                            .map(StreamInner::$HostVariant)
                            .map(Stream),
                    )*
                }
            }
        }

        impl crate::traits::HostTrait for Host {
            type Devices = Devices;
            type Device = Device;

            fn is_available() -> bool {
                $( crate::host::$host_mod::Host::is_available() ||)* false
            }

            fn devices(&self) -> Result<Self::Devices, crate::DevicesError> {
                match self.0 {
                    $(
                        HostInner::$HostVariant(ref h) => {
                            h.devices().map(DevicesInner::$HostVariant).map(Devices)
                        }
                    )*
                }
            }

            fn default_input_device(&self) -> Option<Self::Device> {
                match self.0 {
                    $(
                        HostInner::$HostVariant(ref h) => {
                            h.default_input_device().map(DeviceInner::$HostVariant).map(Device)
                        }
                    )*
                }
            }

            fn default_output_device(&self) -> Option<Self::Device> {
                match self.0 {
                    $(
                        HostInner::$HostVariant(ref h) => {
                            h.default_output_device().map(DeviceInner::$HostVariant).map(Device)
                        }
                    )*
                }
            }
        }

        impl crate::traits::StreamTrait for Stream {
            fn play(&self) -> Result<(), crate::PlayStreamError> {
                match self.0 {
                    $(
                        StreamInner::$HostVariant(ref s) => {
                            s.play()
                        }
                    )*
                }
            }

            fn pause(&self) -> Result<(), crate::PauseStreamError> {
                match self.0 {
                    $(
                        StreamInner::$HostVariant(ref s) => {
                            s.pause()
                        }
                    )*
                }
            }
        }

        $(
            impl From<crate::host::$host_mod::Device> for Device {
                fn from(h: crate::host::$host_mod::Device) -> Self {
                    Device(DeviceInner::$HostVariant(h))
                }
            }

            impl From<crate::host::$host_mod::Devices> for Devices {
                fn from(h: crate::host::$host_mod::Devices) -> Self {
                    Devices(DevicesInner::$HostVariant(h))
                }
            }

            impl From<crate::host::$host_mod::Host> for Host {
                fn from(h: crate::host::$host_mod::Host) -> Self {
                    Host(HostInner::$HostVariant(h))
                }
            }

            impl From<crate::host::$host_mod::Stream> for Stream {
                fn from(h: crate::host::$host_mod::Stream) -> Self {
                    Stream(StreamInner::$HostVariant(h))
                }
            }
        )*

        /// Produces a list of hosts that are currently available on the system.
        pub fn available_hosts() -> Vec<HostId> {
            let mut host_ids = vec![];
            $(
                if <crate::host::$host_mod::Host as crate::traits::HostTrait>::is_available() {
                    host_ids.push(HostId::$HostVariant);
                }
            )*
            host_ids
        }

        /// Given a unique host identifier, initialise and produce the host if it is available.
        pub fn host_from_id(id: HostId) -> Result<Host, crate::HostUnavailable> {
            match id {
                $(
                    HostId::$HostVariant => {
                        crate::host::$host_mod::Host::new()
                            .map(HostInner::$HostVariant)
                            .map(Host)
                    }
                )*
            }
        }
    };
}

// TODO: Add pulseaudio and jack here eventually.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod platform_impl {
    pub use crate::host::alsa::{
        Device as AlsaDevice,
        Devices as AlsaDevices,
        Host as AlsaHost,
        Stream as AlsaStream,
        SupportedInputFormats as AlsaSupportedInputFormats,
        SupportedOutputFormats as AlsaSupportedOutputFormats,
    };

    impl_platform_host!(Alsa alsa "ALSA");

    /// The default host for the current compilation target platform.
    pub fn default_host() -> Host {
        AlsaHost::new()
            .expect("the default host should always be available")
            .into()
    }
}


#[cfg(any(target_os = "macos", target_os = "ios"))]
mod platform_impl {
    pub use crate::host::coreaudio::{
        Device as CoreAudioDevice,
        Devices as CoreAudioDevices,
        Host as CoreAudioHost,
        Stream as CoreAudioStream,
        SupportedInputFormats as CoreAudioSupportedInputFormats,
        SupportedOutputFormats as CoreAudioSupportedOutputFormats,
    };

    impl_platform_host!(CoreAudio coreaudio "CoreAudio");

    /// The default host for the current compilation target platform.
    pub fn default_host() -> Host {
        CoreAudioHost::new()
            .expect("the default host should always be available")
            .into()
    }
}

#[cfg(target_os = "emscripten")]
mod platform_impl {
    pub use crate::host::emscripten::{
        Device as EmscriptenDevice,
        Devices as EmscriptenDevices,
        Host as EmscriptenHost,
        Stream as EmscriptenStream,
        SupportedInputFormats as EmscriptenSupportedInputFormats,
        SupportedOutputFormats as EmscriptenSupportedOutputFormats,
    };

    impl_platform_host!(Emscripten emscripten "Emscripten");

    /// The default host for the current compilation target platform.
    pub fn default_host() -> Host {
        EmscriptenHost::new()
            .expect("the default host should always be available")
            .into()
    }
}

#[cfg(windows)]
mod platform_impl {
    #[cfg(feature = "asio")]
    pub use crate::host::asio::{
        Device as AsioDevice,
        Devices as AsioDevices,
        Stream as AsioStream,
        Host as AsioHost,
        SupportedInputFormats as AsioSupportedInputFormats,
        SupportedOutputFormats as AsioSupportedOutputFormats,
    };
    pub use crate::host::wasapi::{
        Device as WasapiDevice,
        Devices as WasapiDevices,
        Stream as WasapiStream,
        Host as WasapiHost,
        SupportedInputFormats as WasapiSupportedInputFormats,
        SupportedOutputFormats as WasapiSupportedOutputFormats,
    };

    #[cfg(feature = "asio")]
    impl_platform_host!(Asio asio "ASIO", Wasapi wasapi "WASAPI");

    #[cfg(not(feature = "asio"))]
    impl_platform_host!(Wasapi wasapi "WASAPI");

    /// The default host for the current compilation target platform.
    pub fn default_host() -> Host {
        WasapiHost::new()
            .expect("the default host should always be available")
            .into()
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "freebsd", target_os = "macos",
              target_os = "ios", target_os = "emscripten")))]
mod platform_impl {
    pub use crate::host::null::{
        Device as NullDevice,
        Devices as NullDevices,
        EventLoop as NullEventLoop,
        Host as NullHost,
        StreamId as NullStreamId,
        SupportedInputFormats as NullSupportedInputFormats,
        SupportedOutputFormats as NullSupportedOutputFormats,
    };

    impl_platform_host!(Null null "Null");

    /// The default host for the current compilation target platform.
    pub fn default_host() -> Host {
        NullHost::new()
            .expect("the default host should always be available")
            .into()
    }
}
