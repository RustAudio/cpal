//! Platform-specific items.
//!
//! This module also contains the implementation of the platform's dynamically dispatched [`Host`]
//! type and its associated [`Device`], [`Stream`] and other associated types. These
//! types are useful in the case that users require switching between audio host APIs at runtime.

#[doc(inline)]
pub use self::platform_impl::*;

/// A macro to assist with implementing a platform's dynamically dispatched [`Host`] type.
///
/// These dynamically dispatched types are necessary to allow for users to switch between hosts at
/// runtime.
///
/// For example the invocation `impl_platform_host(Wasapi wasapi "WASAPI", Asio asio "ASIO")`,
/// this macro should expand to:
///
// This sample code block is marked as text because it's not a valid test,
// it's just illustrative. (see rust issue #96573)
/// ```text
/// pub enum HostId {
///     Wasapi,
///     Asio,
/// }
///
/// pub enum Host {
///     Wasapi(crate::host::wasapi::Host),
///     Asio(crate::host::asio::Host),
/// }
/// ```
///
/// And so on for Device, Devices, Host, Stream, SupportedInputConfigs,
/// SupportedOutputConfigs and all their necessary trait implementations.
///
macro_rules! impl_platform_host {
    ($($(#[cfg($feat: meta)])? $HostVariant:ident $host_mod:ident $host_name:literal),*) => {
        /// All hosts supported by CPAL on this platform.
        pub const ALL_HOSTS: &'static [HostId] = &[
            $(
                $(#[cfg($feat)])?
                HostId::$HostVariant,
            )*
        ];

        /// The platform's dynamically dispatched `Host` type.
        ///
        /// An instance of this `Host` type may represent one of the `Host`s available
        /// on the platform.
        ///
        /// Use this type if you require switching between available hosts at runtime.
        ///
        /// This type may be constructed via the [`host_from_id`] function. [`HostId`]s may
        /// be acquired via the [`ALL_HOSTS`] const, and the [`available_hosts`] function.
        pub struct Host(HostInner);

        /// The `Device` implementation associated with the platform's dynamically dispatched
        /// [`Host`] type.
        #[derive(Clone)]
        pub struct Device(DeviceInner);

        /// The `Devices` iterator associated with the platform's dynamically dispatched [`Host`]
        /// type.
        pub struct Devices(DevicesInner);

        /// The `Stream` implementation associated with the platform's dynamically dispatched
        /// [`Host`] type.
        // Streams cannot be `Send` or `Sync` if we plan to support Android's AAudio API. This is
        // because the stream API is not thread-safe, and the API prohibits calling certain
        // functions within the callback.
        //
        // TODO: Confirm this and add more specific detail and references.
        #[must_use = "If the stream is not stored it will not play."]
        pub struct Stream(StreamInner, crate::platform::NotSendSyncAcrossAllPlatforms);

        /// The `SupportedInputConfigs` iterator associated with the platform's dynamically
        /// dispatched [`Host`] type.
        pub struct SupportedInputConfigs(SupportedInputConfigsInner);

        /// The `SupportedOutputConfigs` iterator associated with the platform's dynamically
        /// dispatched [`Host`] type.
        pub struct SupportedOutputConfigs(SupportedOutputConfigsInner);

        /// Unique identifier for available hosts on the platform.
        #[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
        pub enum HostId {
            $(
                $(#[cfg($feat)])?
                $HostVariant,
            )*
        }

        /// Contains a platform specific [`Device`] implementation.
        #[derive(Clone)]
        pub enum DeviceInner {
            $(
                $(#[cfg($feat)])?
                $HostVariant(crate::host::$host_mod::Device),
            )*
        }

        /// Contains a platform specific [`Devices`] implementation.
        pub enum DevicesInner {
            $(
                $(#[cfg($feat)])?
                $HostVariant(crate::host::$host_mod::Devices),
            )*
        }

        /// Contains a platform specific [`Host`] implementation.
        pub enum HostInner {
            $(
                $(#[cfg($feat)])?
                $HostVariant(crate::host::$host_mod::Host),
            )*
        }

        /// Contains a platform specific [`Stream`] implementation.
        pub enum StreamInner {
            $(
                $(#[cfg($feat)])?
                $HostVariant(crate::host::$host_mod::Stream),
            )*
        }

        enum SupportedInputConfigsInner {
            $(
                $(#[cfg($feat)])?
                $HostVariant(crate::host::$host_mod::SupportedInputConfigs),
            )*
        }

        enum SupportedOutputConfigsInner {
            $(
                $(#[cfg($feat)])?
                $HostVariant(crate::host::$host_mod::SupportedOutputConfigs),
            )*
        }

        impl HostId {
            pub fn name(&self) -> &'static str {
                match self {
                    $(
                        $(#[cfg($feat)])?
                        HostId::$HostVariant => $host_name,
                    )*
                }
            }
        }

        impl Devices {
            /// Returns a reference to the underlying platform specific implementation of this
            /// `Devices`.
            pub fn as_inner(&self) -> &DevicesInner {
                &self.0
            }

            /// Returns a mutable reference to the underlying platform specific implementation of
            /// this `Devices`.
            pub fn as_inner_mut(&mut self) -> &mut DevicesInner {
                &mut self.0
            }

            /// Returns the underlying platform specific implementation of this `Devices`.
            pub fn into_inner(self) -> DevicesInner {
                self.0
            }
        }

        impl Device {
            /// Returns a reference to the underlying platform specific implementation of this
            /// `Device`.
            pub fn as_inner(&self) -> &DeviceInner {
                &self.0
            }

            /// Returns a mutable reference to the underlying platform specific implementation of
            /// this `Device`.
            pub fn as_inner_mut(&mut self) -> &mut DeviceInner {
                &mut self.0
            }

            /// Returns the underlying platform specific implementation of this `Device`.
            pub fn into_inner(self) -> DeviceInner {
                self.0
            }
        }

        impl Host {
            /// The unique identifier associated with this `Host`.
            pub fn id(&self) -> HostId {
                match self.0 {
                    $(
                        $(#[cfg($feat)])?
                        HostInner::$HostVariant(_) => HostId::$HostVariant,
                    )*
                }
            }

            /// Returns a reference to the underlying platform specific implementation of this
            /// `Host`.
            pub fn as_inner(&self) -> &HostInner {
                &self.0
            }

            /// Returns a mutable reference to the underlying platform specific implementation of
            /// this `Host`.
            pub fn as_inner_mut(&mut self) -> &mut HostInner {
                &mut self.0
            }

            /// Returns the underlying platform specific implementation of this `Host`.
            pub fn into_inner(self) -> HostInner {
                self.0
            }
        }

        impl Stream {
            /// Returns a reference to the underlying platform specific implementation of this
            /// `Stream`.
            pub fn as_inner(&self) -> &StreamInner {
                &self.0
            }

            /// Returns a mutable reference to the underlying platform specific implementation of
            /// this `Stream`.
            pub fn as_inner_mut(&mut self) -> &mut StreamInner {
                &mut self.0
            }

            /// Returns the underlying platform specific implementation of this `Stream`.
            pub fn into_inner(self) -> StreamInner {
                self.0
            }
        }

        impl Iterator for Devices {
            type Item = Device;

            fn next(&mut self) -> Option<Self::Item> {
                match self.0 {
                    $(
                        $(#[cfg($feat)])?
                        DevicesInner::$HostVariant(ref mut d) => {
                            d.next().map(DeviceInner::$HostVariant).map(Device::from)
                        }
                    )*
                }
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                match self.0 {
                    $(
                        $(#[cfg($feat)])?
                        DevicesInner::$HostVariant(ref d) => d.size_hint(),
                    )*
                }
            }
        }

        impl Iterator for SupportedInputConfigs {
            type Item = crate::SupportedStreamConfigRange;

            fn next(&mut self) -> Option<Self::Item> {
                match self.0 {
                    $(
                        $(#[cfg($feat)])?
                        SupportedInputConfigsInner::$HostVariant(ref mut s) => s.next(),
                    )*
                }
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                match self.0 {
                    $(
                        $(#[cfg($feat)])?
                        SupportedInputConfigsInner::$HostVariant(ref d) => d.size_hint(),
                    )*
                }
            }
        }

        impl Iterator for SupportedOutputConfigs {
            type Item = crate::SupportedStreamConfigRange;

            fn next(&mut self) -> Option<Self::Item> {
                match self.0 {
                    $(
                        $(#[cfg($feat)])?
                        SupportedOutputConfigsInner::$HostVariant(ref mut s) => s.next(),
                    )*
                }
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                match self.0 {
                    $(
                        $(#[cfg($feat)])?
                        SupportedOutputConfigsInner::$HostVariant(ref d) => d.size_hint(),
                    )*
                }
            }
        }

        impl crate::traits::DeviceTrait for Device {
            type SupportedInputConfigs = SupportedInputConfigs;
            type SupportedOutputConfigs = SupportedOutputConfigs;
            type Stream = Stream;

            fn name(&self) -> Result<String, crate::DeviceNameError> {
                match self.0 {
                    $(
                        $(#[cfg($feat)])?
                        DeviceInner::$HostVariant(ref d) => d.name(),
                    )*
                }
            }

            fn supported_input_configs(&self) -> Result<Self::SupportedInputConfigs, crate::SupportedStreamConfigsError> {
                match self.0 {
                    $(
                        $(#[cfg($feat)])?
                        DeviceInner::$HostVariant(ref d) => {
                            d.supported_input_configs()
                                .map(SupportedInputConfigsInner::$HostVariant)
                                .map(SupportedInputConfigs)
                        }
                    )*
                }
            }

            fn supported_output_configs(&self) -> Result<Self::SupportedOutputConfigs, crate::SupportedStreamConfigsError> {
                match self.0 {
                    $(
                        $(#[cfg($feat)])?
                        DeviceInner::$HostVariant(ref d) => {
                            d.supported_output_configs()
                                .map(SupportedOutputConfigsInner::$HostVariant)
                                .map(SupportedOutputConfigs)
                        }
                    )*
                }
            }

            fn default_input_config(&self) -> Result<crate::SupportedStreamConfig, crate::DefaultStreamConfigError> {
                match self.0 {
                    $(
                        $(#[cfg($feat)])?
                        DeviceInner::$HostVariant(ref d) => d.default_input_config(),
                    )*
                }
            }

            fn default_output_config(&self) -> Result<crate::SupportedStreamConfig, crate::DefaultStreamConfigError> {
                match self.0 {
                    $(
                        $(#[cfg($feat)])?
                        DeviceInner::$HostVariant(ref d) => d.default_output_config(),
                    )*
                }
            }

            fn build_input_stream_raw<D, E>(
                &self,
                config: &crate::StreamConfig,
                sample_format: crate::SampleFormat,
                data_callback: D,
                error_callback: E,
                timeout: Option<std::time::Duration>,
            ) -> Result<Self::Stream, crate::BuildStreamError>
            where
                D: FnMut(&crate::Data, &crate::InputCallbackInfo) + Send + 'static,
                E: FnMut(crate::StreamError) + Send + 'static,
            {
                match self.0 {
                    $(
                        $(#[cfg($feat)])?
                        DeviceInner::$HostVariant(ref d) => d
                            .build_input_stream_raw(
                                config,
                                sample_format,
                                data_callback,
                                error_callback,
                                timeout,
                            )
                            .map(StreamInner::$HostVariant)
                            .map(Stream::from),
                    )*
                }
            }

            fn build_output_stream_raw<D, E>(
                &self,
                config: &crate::StreamConfig,
                sample_format: crate::SampleFormat,
                data_callback: D,
                error_callback: E,
                timeout: Option<std::time::Duration>,
            ) -> Result<Self::Stream, crate::BuildStreamError>
            where
                D: FnMut(&mut crate::Data, &crate::OutputCallbackInfo) + Send + 'static,
                E: FnMut(crate::StreamError) + Send + 'static,
            {
                match self.0 {
                    $(
                        $(#[cfg($feat)])?
                        DeviceInner::$HostVariant(ref d) => d
                            .build_output_stream_raw(
                                config,
                                sample_format,
                                data_callback,
                                error_callback,
                                timeout,
                            )
                            .map(StreamInner::$HostVariant)
                            .map(Stream::from),
                    )*
                }
            }
        }

        impl crate::traits::HostTrait for Host {
            type Devices = Devices;
            type Device = Device;

            fn is_available() -> bool {
                $(
                    $(#[cfg($feat)])?
                    if crate::host::$host_mod::Host::is_available() { return true; }
                )*
                false
            }

            fn devices(&self) -> Result<Self::Devices, crate::DevicesError> {
                match self.0 {
                    $(
                        $(#[cfg($feat)])?
                        HostInner::$HostVariant(ref h) => {
                            h.devices().map(DevicesInner::$HostVariant).map(Devices::from)
                        }
                    )*
                }
            }

            fn default_input_device(&self) -> Option<Self::Device> {
                match self.0 {
                    $(
                        $(#[cfg($feat)])?
                        HostInner::$HostVariant(ref h) => {
                            h.default_input_device().map(DeviceInner::$HostVariant).map(Device::from)
                        }
                    )*
                }
            }

            fn default_output_device(&self) -> Option<Self::Device> {
                match self.0 {
                    $(
                        $(#[cfg($feat)])?
                        HostInner::$HostVariant(ref h) => {
                            h.default_output_device().map(DeviceInner::$HostVariant).map(Device::from)
                        }
                    )*
                }
            }
        }

        impl crate::traits::StreamTrait for Stream {
            fn play(&self) -> Result<(), crate::PlayStreamError> {
                match self.0 {
                    $(
                        $(#[cfg($feat)])?
                        StreamInner::$HostVariant(ref s) => {
                            s.play()
                        }
                    )*
                }
            }

            fn pause(&self) -> Result<(), crate::PauseStreamError> {
                match self.0 {
                    $(
                        $(#[cfg($feat)])?
                        StreamInner::$HostVariant(ref s) => {
                            s.pause()
                        }
                    )*
                }
            }
        }

        impl From<DeviceInner> for Device {
            fn from(d: DeviceInner) -> Self {
                Device(d)
            }
        }

        impl From<DevicesInner> for Devices {
            fn from(d: DevicesInner) -> Self {
                Devices(d)
            }
        }

        impl From<HostInner> for Host {
            fn from(h: HostInner) -> Self {
                Host(h)
            }
        }

        impl From<StreamInner> for Stream {
            fn from(s: StreamInner) -> Self {
                Stream(s, Default::default())
            }
        }

        $(
            $(#[cfg($feat)])?
            impl From<crate::host::$host_mod::Device> for Device {
                fn from(h: crate::host::$host_mod::Device) -> Self {
                    DeviceInner::$HostVariant(h).into()
                }
            }

            $(#[cfg($feat)])?
            impl From<crate::host::$host_mod::Devices> for Devices {
                fn from(h: crate::host::$host_mod::Devices) -> Self {
                    DevicesInner::$HostVariant(h).into()
                }
            }

            $(#[cfg($feat)])?
            impl From<crate::host::$host_mod::Host> for Host {
                fn from(h: crate::host::$host_mod::Host) -> Self {
                    HostInner::$HostVariant(h).into()
                }
            }

            $(#[cfg($feat)])?
            impl From<crate::host::$host_mod::Stream> for Stream {
                fn from(h: crate::host::$host_mod::Stream) -> Self {
                    StreamInner::$HostVariant(h).into()
                }
            }
        )*

        /// Produces a list of hosts that are currently available on the system.
        pub fn available_hosts() -> Vec<HostId> {
            let mut host_ids = vec![];
            $(
                $(#[cfg($feat)])?
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
                    $(#[cfg($feat)])?
                    HostId::$HostVariant => {
                        crate::host::$host_mod::Host::new()
                            .map(HostInner::$HostVariant)
                            .map(Host::from)
                    }
                )*
            }
        }
    };
}

// TODO: Add pulseaudio and jack here eventually.
#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd"
))]
mod platform_impl {
    pub use crate::host::alsa::{
        Device as AlsaDevice, Devices as AlsaDevices, Host as AlsaHost, Stream as AlsaStream,
        SupportedInputConfigs as AlsaSupportedInputConfigs,
        SupportedOutputConfigs as AlsaSupportedOutputConfigs,
    };
    #[cfg(feature = "jack")]
    pub use crate::host::jack::{
        Device as JackDevice, Devices as JackDevices, Host as JackHost, Stream as JackStream,
        SupportedInputConfigs as JackSupportedInputConfigs,
        SupportedOutputConfigs as JackSupportedOutputConfigs,
    };

    impl_platform_host!(#[cfg(feature = "jack")] Jack jack "JACK", Alsa alsa "ALSA");

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
        Device as CoreAudioDevice, Devices as CoreAudioDevices, Host as CoreAudioHost,
        Stream as CoreAudioStream, SupportedInputConfigs as CoreAudioSupportedInputConfigs,
        SupportedOutputConfigs as CoreAudioSupportedOutputConfigs,
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
        Device as EmscriptenDevice, Devices as EmscriptenDevices, Host as EmscriptenHost,
        Stream as EmscriptenStream, SupportedInputConfigs as EmscriptenSupportedInputConfigs,
        SupportedOutputConfigs as EmscriptenSupportedOutputConfigs,
    };

    impl_platform_host!(Emscripten emscripten "Emscripten");

    /// The default host for the current compilation target platform.
    pub fn default_host() -> Host {
        EmscriptenHost::new()
            .expect("the default host should always be available")
            .into()
    }
}

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
mod platform_impl {
    pub use crate::host::webaudio::{
        Device as WebAudioDevice, Devices as WebAudioDevices, Host as WebAudioHost,
        Stream as WebAudioStream, SupportedInputConfigs as WebAudioSupportedInputConfigs,
        SupportedOutputConfigs as WebAudioSupportedOutputConfigs,
    };

    impl_platform_host!(WebAudio webaudio "WebAudio");

    /// The default host for the current compilation target platform.
    pub fn default_host() -> Host {
        WebAudioHost::new()
            .expect("the default host should always be available")
            .into()
    }
}

#[cfg(windows)]
mod platform_impl {
    #[cfg(feature = "asio")]
    pub use crate::host::asio::{
        Device as AsioDevice, Devices as AsioDevices, Host as AsioHost, Stream as AsioStream,
        SupportedInputConfigs as AsioSupportedInputConfigs,
        SupportedOutputConfigs as AsioSupportedOutputConfigs,
    };
    pub use crate::host::wasapi::{
        Device as WasapiDevice, Devices as WasapiDevices, Host as WasapiHost,
        Stream as WasapiStream, SupportedInputConfigs as WasapiSupportedInputConfigs,
        SupportedOutputConfigs as WasapiSupportedOutputConfigs,
    };

    impl_platform_host!(#[cfg(feature = "asio")] Asio asio "ASIO", Wasapi wasapi "WASAPI");

    /// The default host for the current compilation target platform.
    pub fn default_host() -> Host {
        WasapiHost::new()
            .expect("the default host should always be available")
            .into()
    }
}

#[cfg(target_os = "android")]
mod platform_impl {
    pub use crate::host::oboe::{
        Device as OboeDevice, Devices as OboeDevices, Host as OboeHost, Stream as OboeStream,
        SupportedInputConfigs as OboeSupportedInputConfigs,
        SupportedOutputConfigs as OboeSupportedOutputConfigs,
    };

    impl_platform_host!(Oboe oboe "Oboe");

    /// The default host for the current compilation target platform.
    pub fn default_host() -> Host {
        OboeHost::new()
            .expect("the default host should always be available")
            .into()
    }
}

#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "macos",
    target_os = "ios",
    target_os = "emscripten",
    target_os = "android",
    all(target_arch = "wasm32", feature = "wasm-bindgen"),
)))]
mod platform_impl {
    pub use crate::host::null::{
        Device as NullDevice, Devices as NullDevices, Host as NullHost,
        SupportedInputConfigs as NullSupportedInputConfigs,
        SupportedOutputConfigs as NullSupportedOutputConfigs,
    };

    impl_platform_host!(Null null "Null");

    /// The default host for the current compilation target platform.
    pub fn default_host() -> Host {
        NullHost::new()
            .expect("the default host should always be available")
            .into()
    }
}

// The following zero-sized types are for applying Send/Sync restrictions to ensure
// consistent behaviour across different platforms. These verbosely named types are used
// (rather than using the markers directly) in the hope of making the compile errors
// slightly more helpful.
//
// TODO: Remove these in favour of using negative trait bounds if they stabilise.

// A marker used to remove the `Send` and `Sync` traits.
struct NotSendSyncAcrossAllPlatforms(std::marker::PhantomData<*mut ()>);

impl Default for NotSendSyncAcrossAllPlatforms {
    fn default() -> Self {
        NotSendSyncAcrossAllPlatforms(std::marker::PhantomData)
    }
}
