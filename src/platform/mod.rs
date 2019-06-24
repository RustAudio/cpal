//! Platform-specific items.
//!
//! This module also contains the implementation of the platform's dynamically dispatched `Host`
//! type and its associated `EventLoop`, `Device`, `StreamId` and other associated types. These
//! types are useful in the case that users require switching between audio host APIs at runtime.

// A macro to assist with implementing a platform's dynamically dispatched `Host` type.
//
// These dynamically dispatched types are necessary to allow for users to switch between hosts at
// runtime.
//
// For example the invocation `impl_platform_host(Wasapi wasapi, Asio asio)`, this macro should
// expand to:
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
    ($($HostVariant:ident $host_mod:ident),*) => {
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

        /// The **EventLoop** implementation associated with the platform's dynamically dispatched
        /// **Host** type.
        pub struct EventLoop(EventLoopInner);

        /// The **StreamId** implementation associated with the platform's dynamically dispatched
        /// **Host** type.
        #[derive(Clone, Debug, Eq, PartialEq)]
        pub struct StreamId(StreamIdInner);

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

        enum EventLoopInner {
            $(
                $HostVariant(crate::host::$host_mod::EventLoop),
            )*
        }

        enum HostInner {
            $(
                $HostVariant(crate::host::$host_mod::Host),
            )*
        }

        #[derive(Clone, Debug, Eq, PartialEq)]
        enum StreamIdInner {
            $(
                $HostVariant(crate::host::$host_mod::StreamId),
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

        impl crate::Device for Device {
            type SupportedInputFormats = SupportedInputFormats;
            type SupportedOutputFormats = SupportedOutputFormats;

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
        }

        impl crate::EventLoop for EventLoop {
            type StreamId = StreamId;
            type Device = Device;

            fn build_input_stream(
                &self,
                device: &Self::Device,
                format: &crate::Format,
            ) -> Result<Self::StreamId, crate::BuildStreamError> {
                match (&self.0, &device.0) {
                    $(
                        (&EventLoopInner::$HostVariant(ref e), &DeviceInner::$HostVariant(ref d)) => {
                            e.build_input_stream(d, format)
                                .map(StreamIdInner::$HostVariant)
                                .map(StreamId)
                        }
                    )*
                }
            }

            fn build_output_stream(
                &self,
                device: &Self::Device,
                format: &crate::Format,
            ) -> Result<Self::StreamId, crate::BuildStreamError> {
                match (&self.0, &device.0) {
                    $(
                        (&EventLoopInner::$HostVariant(ref e), &DeviceInner::$HostVariant(ref d)) => {
                            e.build_output_stream(d, format)
                                .map(StreamIdInner::$HostVariant)
                                .map(StreamId)
                        }
                    )*
                }
            }

            fn play_stream(&self, stream: Self::StreamId) -> Result<(), crate::PlayStreamError> {
                match (&self.0, stream.0) {
                    $(
                        (&EventLoopInner::$HostVariant(ref e), StreamIdInner::$HostVariant(ref s)) => {
                            e.play_stream(s.clone())
                        }
                    )*
                }
            }

            fn pause_stream(&self, stream: Self::StreamId) -> Result<(), crate::PauseStreamError> {
                match (&self.0, stream.0) {
                    $(
                        (&EventLoopInner::$HostVariant(ref e), StreamIdInner::$HostVariant(ref s)) => {
                            e.pause_stream(s.clone())
                        }
                    )*
                }
            }

            fn destroy_stream(&self, stream: Self::StreamId) {
                match (&self.0, stream.0) {
                    $(
                        (&EventLoopInner::$HostVariant(ref e), StreamIdInner::$HostVariant(ref s)) => {
                            e.destroy_stream(s.clone())
                        }
                    )*
                }
            }

            fn run<F>(&self, mut callback: F) -> !
            where
                F: FnMut(Self::StreamId, crate::StreamDataResult) + Send
            {
                match self.0 {
                    $(
                        EventLoopInner::$HostVariant(ref e) => {
                            e.run(|id, result| {
                                let result = result;
                                callback(StreamId(StreamIdInner::$HostVariant(id)), result);
                            });
                        },
                    )*
                }
            }
        }

        impl crate::Host for Host {
            type Devices = Devices;
            type Device = Device;
            type EventLoop = EventLoop;

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

            fn event_loop(&self) -> Self::EventLoop {
                match self.0 {
                    $(
                        HostInner::$HostVariant(ref h) => {
                            EventLoop(EventLoopInner::$HostVariant(h.event_loop()))
                        }
                    )*
                }
            }
        }

        impl crate::StreamId for StreamId {}

        /// Produces a list of hosts that are currently available on the system.
        pub fn available_hosts() -> Vec<HostId> {
            let mut host_ids = vec![];
            $(
                if <crate::host::$host_mod::Host as crate::Host>::is_available() {
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
impl_platform_host!(Alsa alsa);

#[cfg(any(target_os = "macos", target_os = "ios"))]
impl_platform_host!(CoreAudio coreaudio);

#[cfg(target_os = "emscripten")]
impl_platform_host!(Emscripten emscripten);

// TODO: Add `Asio asio` once #221 lands.
#[cfg(windows)]
impl_platform_host!(Wasapi wasapi);

#[cfg(not(any(windows, target_os = "linux", target_os = "freebsd", target_os = "macos",
              target_os = "ios", target_os = "emscripten")))]
impl_platform_host!(Null null);

/// The default host for the current compilation target platform.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub type DefaultHost = crate::host::alsa::Host;

/// The default host for the current compilation target platform.
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub type DefaultHost = crate::host::coreaudio::Host;

/// The default host for the current compilation target platform.
#[cfg(target_os = "emscripten")]
pub type DefaultHost = crate::host::emscripten::Host;

#[cfg(not(any(windows, target_os = "linux", target_os = "freebsd", target_os = "macos",
              target_os = "ios", target_os = "emscripten")))]
pub type DefaultHost = crate::host::null::Host;

/// The default host for the current compilation target platform.
#[cfg(windows)]
pub type DefaultHost = crate::host::wasapi::Host;

/// Retrieve the default host for the system.
///
/// There should *always* be a default host for each of the supported target platforms, regardless
/// of whether or not there are any available audio devices.
pub fn default_host() -> DefaultHost {
    DefaultHost::new().expect("the default host should always be available")
}
