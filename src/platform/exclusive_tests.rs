use super::*;

#[cfg(feature = "custom")]
mod custom_backend {
    use super::*;
    use crate::traits::DeviceTrait;
    use std::collections::hash_map::DefaultHasher;
    use std::fmt;
    use std::hash::{Hash, Hasher};
    use std::time::Duration;

    #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    struct FakeDevice;

    impl fmt::Display for FakeDevice {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("fake exclusive device")
        }
    }

    impl DeviceTrait for FakeDevice {
        type SupportedInputConfigs = std::iter::Empty<crate::SupportedStreamConfigRange>;
        type SupportedOutputConfigs = std::iter::Empty<crate::SupportedStreamConfigRange>;
        type Stream = crate::host::custom::Stream;

        fn description(&self) -> Result<crate::DeviceDescription, crate::Error> {
            Ok(crate::DeviceDescriptionBuilder::new("Fake exclusive device").build())
        }

        fn id(&self) -> Result<crate::DeviceId, crate::Error> {
            Ok(crate::DeviceId::new(
                crate::platform::HostId::Custom,
                "fake",
            ))
        }

        fn supported_input_configs(&self) -> Result<Self::SupportedInputConfigs, crate::Error> {
            Ok(std::iter::empty())
        }

        fn supported_output_configs(&self) -> Result<Self::SupportedOutputConfigs, crate::Error> {
            Ok(std::iter::empty())
        }

        fn default_input_config(&self) -> Result<crate::SupportedStreamConfig, crate::Error> {
            Err(crate::Error::new(crate::ErrorKind::UnsupportedOperation))
        }

        fn default_output_config(&self) -> Result<crate::SupportedStreamConfig, crate::Error> {
            Err(crate::Error::new(crate::ErrorKind::UnsupportedOperation))
        }

        fn build_input_stream_raw<D, E>(
            &self,
            _: crate::StreamConfig,
            _: crate::SampleFormat,
            _: D,
            _: E,
            _: Option<Duration>,
        ) -> Result<Self::Stream, crate::Error>
        where
            D: FnMut(&crate::Data, &crate::CallbackInfo) + Send + 'static,
            E: FnMut(crate::Error) + Send + 'static,
        {
            Err(crate::Error::new(crate::ErrorKind::UnsupportedOperation))
        }

        fn build_output_stream_raw<D, E>(
            &self,
            _: crate::StreamConfig,
            _: crate::SampleFormat,
            _: D,
            _: E,
            _: Option<Duration>,
        ) -> Result<Self::Stream, crate::Error>
        where
            D: FnMut(&mut crate::Data, &crate::CallbackInfo) + Send + 'static,
            E: FnMut(crate::Error) + Send + 'static,
        {
            Err(crate::Error::new(crate::ErrorKind::UnsupportedOperation))
        }
    }

    fn device() -> Device {
        Device::from(crate::platform::CustomDevice::from_device(FakeDevice))
    }

    fn unsupported<T>(result: Result<T, crate::Error>) {
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("exclusive operation unexpectedly succeeded"),
        };
        assert_eq!(error.kind(), crate::ErrorKind::UnsupportedOperation);
    }

    #[test]
    fn unsupported_backend_fails_closed_for_exclusive_queries() {
        let exclusive = device().exclusive(true);
        assert!(!exclusive.supports_exclusive());
        assert!(!exclusive.supports_input());
        assert!(!exclusive.supports_output());
        assert!(!exclusive.supports_duplex());
        unsupported(exclusive.supported_input_configs());
        unsupported(exclusive.supported_output_configs());
        unsupported(exclusive.default_input_config());
        unsupported(exclusive.default_output_config());
    }

    #[test]
    fn exclusive_selection_preserves_identity_hash_and_reversal() {
        let shared = device();
        let exclusive = shared.clone().exclusive(true);
        assert_eq!(shared, exclusive);
        assert_eq!(shared.id().unwrap(), exclusive.id().unwrap());

        let hash = |device: &Device| {
            let mut hasher = DefaultHasher::new();
            device.hash(&mut hasher);
            hasher.finish()
        };
        assert_eq!(hash(&shared), hash(&exclusive));
        unsupported(exclusive.clone().default_output_config());
        assert!(exclusive.clone().exclusive(false).supports_output() == shared.supports_output());
        assert!(exclusive.exclusive(false).default_output_config().is_err());
    }
}
