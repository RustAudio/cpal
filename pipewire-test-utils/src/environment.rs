use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;
use bollard::{Docker, API_DEFAULT_VERSION};
use ctor::{ctor, dtor};
use libc::{atexit, signal, SIGABRT, SIGINT, SIGSEGV, SIGTERM};
use tokio::runtime::{Runtime};
use pipewire_common::error::Error;
use url::Url;
use crate::containers::container::{ContainerApi, ContainerRegistry, ImageApi, ImageRegistry};
use crate::containers::options::{Size};
use crate::containers::sync_api::SyncContainerApi;
use crate::server::{server_with_default_configuration, Server};

pub static SHARED_SERVER: LazyLock<Arc<Server>> = LazyLock::new(move || {
    let server = server_with_default_configuration();
    server
});

pub static TEST_ENVIRONMENT: LazyLock<Mutex<Environment>> = LazyLock::new(|| {
    unsafe {
        signal(SIGINT, cleanup_test_environment as usize);
        signal(SIGTERM, cleanup_test_environment as usize);
    }
    unsafe { libc::printf("Initialize test environment\n\0".as_ptr() as *const i8); };
    Mutex::new(Environment::from_env())
});

#[dtor]
unsafe fn cleanup_test_environment() {
    libc::printf("Cleaning test environment\n\0".as_ptr() as *const i8);
    let environment = TEST_ENVIRONMENT.lock().unwrap();
    environment.container_image_registry.cleanup();
    SHARED_SERVER.cleanup();
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestTarget {
    Local,
    Container,
}

impl From<String> for TestTarget {
    fn from(value: String) -> Self {
        match value.as_str() {
            "local" => TestTarget::Local,
            "container" => TestTarget::Container,
            _ => panic!("Unknown test target {}", value),
        }
    }
}

pub struct Environment {
    pub runtime: Arc<Runtime>,
    pub container_api: ContainerApi,
    pub container_image_api: ImageApi,
    pub container_api_timeout: Duration,
    pub container_registry: ContainerRegistry,
    pub container_image_registry: ImageRegistry,
    pub container_cpu: f64,
    pub container_memory: Size,
    pub container_memory_swap: Size,
    pub test_target: TestTarget,
    pub client_timeout: Duration,
}

impl Environment {
    pub fn from_env() -> Self {        
        let default = Self::default();
        let container_api_timeout = match std::env::var("CONTAINER_API_TIMEOUT") {
            Ok(value) => Self::parse_duration(value),
            Err(_) => default.container_api_timeout
        };
        let container_cpu = match std::env::var("CONTAINER_CPU") {
            Ok(value) => value.parse::<f64>().unwrap(),
            Err(_) => default.container_cpu,
        };
        let container_memory = match std::env::var("CONTAINER_MEMORY") {
            Ok(value) => value.into(),
            Err(_) => default.container_memory
        };
        let container_memory_swap = match std::env::var("CONTAINER_MEMORY_SWAP") {
            Ok(value) => value.into(),
            Err(_) => default.container_memory
        };
        let test_target = match std::env::var("TEST_TARGET") {
            Ok(value) => value.into(),
            Err(_) => default.test_target.clone(),
        };
        Self {
            runtime: default.runtime.clone(),
            container_api: default.container_api,
            container_image_api: default.container_image_api,
            container_api_timeout,
            container_registry: default.container_registry,
            container_image_registry: default.container_image_registry,
            container_cpu,
            container_memory,
            container_memory_swap,
            test_target,
            client_timeout: default.client_timeout,
        }
    }

    fn parse_duration(value: String) -> Duration {
        let value = value.trim();
        let suffix_length = value.strip_suffix("ms")
            .map_or_else(|| 1, |_| 2);
        let suffix_start_index = value.len() - suffix_length;
        let unit = value.get(suffix_start_index..).unwrap();
        let value = value.get(..suffix_start_index)
            .unwrap()
            .parse::<u64>()
            .unwrap();
        match unit {
            "ms" => Duration::from_millis(value),
            "s" => Duration::from_secs(value),
            "m" => Duration::from_secs(value * 60),
            _ => panic!("Invalid unit {:?}. Only ms, s, m are supported.", unit),
        }
    }
    
    fn parse_container_host<T>(
        on_http: impl FnOnce(&String) -> T,
        on_socket: impl FnOnce(&String) -> T,
    ) -> Result<Arc<T>, Error> {
        const DOCKER_HOST_ENVIRONMENT_KEY: &str = "DOCKER_HOST";
        const CONTAINER_HOST_ENVIRONMENT_KEY: &str = "CONTAINER_HOST";
        
        let docker_host = std::env::var(DOCKER_HOST_ENVIRONMENT_KEY);
        let container_host = std::env::var(CONTAINER_HOST_ENVIRONMENT_KEY);
        let host = match (docker_host, container_host) {
            (Ok(value), Ok(_)) => value,
            (Ok(value), Err(_)) => value,
            (Err(_), Ok(value)) => value,
            (Err(_), Err(_)) => return Err(Error {
                description: format!(
                    "${} or ${} should be set.",
                    DOCKER_HOST_ENVIRONMENT_KEY, CONTAINER_HOST_ENVIRONMENT_KEY
                )
            }),
        };
        let host_url = Url::parse(host.as_str()).unwrap();
        let api = match host_url.scheme() {
            "http" | "tcp" => on_http(&host),
            "unix" => on_socket(&host),
            _ => return Err(Error {
                description: format!("Unsupported uri format {}", host_url),
            }),
        };
        Ok(Arc::new(api))
    }
    
    pub fn create_runtime() -> Arc<Runtime> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .max_blocking_threads(1)
            .worker_threads(1)
            .build()
            .unwrap();
        Arc::new(runtime)
    }
    
    pub fn create_docker_api(timeout: &Duration) -> Arc<Docker> {
        let on_http = |host: &String| {
            Docker::connect_with_http(
                host.as_str(),
                timeout.as_secs(),
                API_DEFAULT_VERSION
            ).unwrap()
        };
        let on_socket = |host: &String| {
            Docker::connect_with_unix(
                host.as_str(),
                timeout.as_secs(),
                API_DEFAULT_VERSION
            ).unwrap()
        };
        match Self::parse_container_host(on_http, on_socket) {
            Ok(value) => value,
            Err(value) => panic!("{}", value),
        }
    }

    pub fn create_docker_sync_api(timeout: &Duration) -> Arc<SyncContainerApi> {
        let on_http = |host: &String| {
            SyncContainerApi::connect_with_http(
                host.as_str(),
                timeout.as_secs(),
                API_DEFAULT_VERSION
            ).unwrap()
        };
        let on_socket = |host: &String| {
            SyncContainerApi::connect_with_unix(
                host.as_str(),
                timeout.as_secs(),
                API_DEFAULT_VERSION
            ).unwrap()
        };
        match Self::parse_container_host(on_http, on_socket) {
            Ok(value) => value,
            Err(value) => panic!("{}", value),
        }
    }
    
    pub fn create_container_api(runtime: Arc<Runtime>, api: Arc<Docker>) -> ContainerApi {
        ContainerApi::new(runtime.clone(), api.clone())
    }
}

impl Default for Environment {
    fn default() -> Self {
        let timeout = Duration::from_secs(30);
        let runtime = Self::create_runtime();
        let api_runtime = Self::create_runtime();
        let docker_api = Self::create_docker_api(&timeout);
        let container_api  = Self::create_container_api(api_runtime.clone(), docker_api.clone());
        Self {
            runtime,
            container_api: container_api.clone(),
            container_image_api: ImageApi::new(api_runtime.clone(), docker_api),
            container_api_timeout: timeout,
            container_registry: ContainerRegistry::new(container_api.clone()),
            container_image_registry: ImageRegistry::new(),
            container_cpu: 2.0,
            container_memory: Size::from_mb(100.0),
            container_memory_swap: Size::from_mb(100.0),
            test_target: TestTarget::Container,
            client_timeout: Duration::from_secs(3),
        }
    }
}