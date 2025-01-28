use bollard::container::{Config, StopContainerOptions};
use bollard::models::{HealthConfig, HostConfig, Mount, MountBindOptions, MountTypeEnum, ResourcesUlimits};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Unit {
    Bytes,
    KB,
    MB,
    GB,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    value: f64,
    unit: Unit,
}

impl Size {
    const fn new(value: f64, unit: Unit) -> Self {
        Self { value, unit }
    }

    pub const fn from_bytes(bytes: f64) -> Self {
        Self::new(bytes, Unit::Bytes)
    }

    pub const fn from_kb(kb: f64) -> Self {
        Self::new(kb, Unit::KB)
    }

    pub const fn from_mb(mb: f64) -> Self {
        Self::new(mb, Unit::MB)
    }

    pub const fn from_gb(gb: f64) -> Self {
        Self::new(gb, Unit::GB)
    }
}

impl From<String> for Size {
    fn from(value: String) -> Self {
        let value = value.trim();
        let unit = value.chars().last().unwrap();
        let unit = match unit {
            'b' => Unit::Bytes,
            'k' => Unit::KB,
            'm' => Unit::MB,
            'g' => Unit::GB,
            _ => panic!("Invalid unit {:?}. Only b, k, m, g are supported.", unit),
        };
        let value = value.chars().take(value.len() - 2).collect::<String>();
        let value = value.parse::<f64>().unwrap();
        Self::new(value, unit)
    }
}

impl From<Size> for u64 {
    fn from(value: Size) -> Self {
        match value.unit {
            Unit::Bytes => value.value as u64,
            Unit::KB => (value.value * 1024.0) as u64,
            Unit::MB => (value.value * 1024.0 * 1024.0) as u64,
            Unit::GB => (value.value * 1024.0 * 1024.0 * 1024.0) as u64,
        }
    }
}

impl From<Size> for i64 {
    fn from(value: Size) -> Self {
        let value: u64 = value.into();
        value as i64
    }
}

pub struct CreateContainerOptionsBuilder {
    image: Option<String>,
    environment: Option<HashMap<String, String>>,
    volumes: Option<HashMap<String, String>>,
    labels: Option<HashMap<String, String>>,
    entrypoint: Option<Vec<String>>,
    healthcheck: Option<Vec<String>>,
    cpus: Option<f64>,
    memory_swap: Option<Size>,
    memory: Option<Size>
}

impl Default for CreateContainerOptionsBuilder {
    fn default() -> Self {
        Self {
            image: None,
            environment: None,
            volumes: None,
            labels: None,
            entrypoint: None,
            healthcheck: None,
            cpus: None,
            memory_swap: None,
            memory: None,
        }
    }
}

impl CreateContainerOptionsBuilder {
    pub fn with_image(&mut self, image: impl Into<String>) -> &mut Self {
        self.image = Some(image.into());
        self
    }

    pub fn with_environment(&mut self, key: impl Into<String>, value: impl Into<String>) -> &mut Self {
        if let None = self.environment {
            self.environment = Some(HashMap::new());
        }
        if let Some(environment) = self.environment.as_mut() {
            environment.insert(key.into(), value.into());
        }
        self
    }

    pub fn with_volume(&mut self, name: impl Into<String>, container_path: impl Into<String>) -> &mut Self {
        if let None = self.volumes {
            self.volumes = Some(HashMap::new());
        }
        if let Some(volumes) = self.volumes.as_mut() {
            volumes.insert(name.into(), container_path.into());
        }
        self
    }

    pub fn with_label(&mut self, key: impl Into<String>, value: impl Into<String>) -> &mut Self {
        if let None = self.labels {
            self.labels = Some(HashMap::new());
        }
        if let Some(labels) = self.labels.as_mut() {
            labels.insert(key.into(), value.into());
        }
        self
    }

    pub fn with_entrypoint(&mut self, value: impl Into<String>) -> &mut Self {
        if let None = self.entrypoint {
            self.entrypoint = Some(Vec::new());
        }
        if let Some(entrypoint) = self.entrypoint.as_mut() {
            let mut value = value.into().split(" ")
                .map(|s| s.to_string())
                .collect::<Vec<_>>();
            entrypoint.append(&mut value);
        }
        self
    }

    pub fn with_healthcheck_command(&mut self, value: impl Into<String>) -> &mut Self {
        if let None = self.healthcheck {
            self.healthcheck = Some(Vec::new());
        }
        if let Some(healthcheck) = self.healthcheck.as_mut() {
            healthcheck.clear();
            healthcheck.push("CMD".to_string());
            healthcheck.push(value.into());
        }
        self
    }

    pub fn with_healthcheck_command_shell(&mut self, value: impl Into<String>) -> &mut Self {
        if let None = self.healthcheck {
            self.healthcheck = Some(Vec::new());
        }
        if let Some(healthcheck) = self.healthcheck.as_mut() {
            healthcheck.clear();
            healthcheck.push("CMD-SHELL".to_string());
            healthcheck.push(value.into());
        }
        self
    }

    pub fn with_cpus(&mut self, cpus: f64) -> &mut Self {
        self.cpus = Some(cpus);
        self
    }

    pub fn with_memory_swap(&mut self, memory_swap: Size) -> &mut Self {
        self.memory_swap = Some(memory_swap);
        self
    }

    pub fn with_memory(&mut self, memory: Size) -> &mut Self {
        self.memory = Some(memory);
        self
    }

    pub fn build(&self) -> Config<String> {
        if self.image.is_none() {
            panic!("Image is required");
        }
        let mut builder = Config::default();
        builder.image = self.image.clone();
        builder.host_config = Some(HostConfig::default());
        if let Some(environment) = self.environment.as_ref() {
            let environment = environment.iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>();
            builder.env = Some(environment);
        }
        if let Some(volumes) = self.volumes.as_ref() {
            let mut volumes = volumes.iter()
                .map(|(k, v)| format!("{}:{}", k, v))
                .collect::<Vec<_>>();
            let host_config = builder.host_config.as_mut().unwrap();
            if let None = host_config.binds {
                host_config.binds = Some(Vec::new())
            }
            if let Some(ref mut binds) = &mut host_config.binds {
                binds.append(&mut volumes)
            }
        }
        if let Some(labels) = self.labels.as_ref() {
            builder.labels = Some(labels.clone());
        }
        if let Some(entrypoint) = self.entrypoint.as_ref() {
            builder.entrypoint = Some(entrypoint.clone());
        }
        if let Some(healthcheck) = self.healthcheck.as_ref() {
            builder.healthcheck = Some(HealthConfig {
                test: Some(healthcheck.clone()),
                ..HealthConfig::default()
            });
        }
        if let Some(cpus) = self.cpus.clone() {
            let host_config = builder.host_config.as_mut().unwrap();
            host_config.nano_cpus = Some((1_000_000_000.0 * cpus) as i64);
        }
        if let Some(memory_swap) = self.memory_swap.clone() {
            let host_config = builder.host_config.as_mut().unwrap();
            host_config.memory_swap = Some(memory_swap.into());
        }
        if let Some(memory) = self.memory.clone() {
            let host_config = builder.host_config.as_mut().unwrap();
            host_config.memory = Some(memory.into());
        }
        builder
    }
}

pub struct StopContainerOptionsBuilder {
    wait: Option<Duration>,
}

impl Default for StopContainerOptionsBuilder {
    fn default() -> Self {
        Self {
            wait: Some(Duration::from_secs(0)),
        }
    }
}

impl StopContainerOptionsBuilder {
    pub fn with_wait(&mut self, time: Duration) -> &mut Self {
        self.wait = Some(time);
        self
    }

    pub fn build(&self) -> StopContainerOptions {
        let mut builder = StopContainerOptions::default();
        builder.t = self.wait.unwrap().as_secs() as i64;
        builder
    }
}