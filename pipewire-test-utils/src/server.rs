use crate::containers::container::{ContainerApi, ImageApi, CONTAINER_PATH, CONTAINER_TMP_PATH};
use crate::containers::options::{CreateContainerOptionsBuilder, StopContainerOptionsBuilder};
use crate::environment::{Environment, TestTarget, TEST_ENVIRONMENT};
use bytes::Bytes;
use pipewire_common::constants::*;
use pipewire_common::impl_callback;
use pipewire_common::error::Error;
use rstest::fixture;
use std::fmt::{Debug, Formatter};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;
use bollard::container::RemoveContainerOptions;
use tar::{Builder, EntryType, Header};
use tokio::runtime::Runtime;
use uuid::Uuid;

static CONTAINER_FILE_PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    CONTAINER_PATH
        .clone()
        .join("pipewire.test.container")
        .as_path()
        .to_path_buf()
});

static PIPEWIRE_SERVICE: LazyLock<Service> = LazyLock::new(move || {
    let service = Service::new(
        "pipewire".to_string(),
        |server| {
            server.spawn("pipewire")
                .build();
        },
        |server| {
            server.wait_for_pipewire()
        },
    );
    service
});

static WIREPLUMBER_SERVICE: LazyLock<Service> = LazyLock::new(move || {
    let service = Service::new(
        "wireplumber".to_string(),
        |server| {
            server.spawn("wireplumber")
                .build();
        },
        |server| {
            server.wait_for_wireplumber()
        },
    );
    service
});

static PULSE_SERVICE: LazyLock<Service> = LazyLock::new(move || {
    let service = Service::new(
        "pulse".to_string(),
        |server| {
            server.spawn("pipewire-pulse")
                .build();
        },
        |server| {
            server.wait_for_pulse();
        }
    );
    service
});

struct SpawnCommandBuilder<'a> {
    configuration: &'a mut Vec<Vec<String>>,
    command: Option<String>,
    arguments: Option<Vec<String>>,
    realtime_priority: Option<u32>,
    user: Option<String>,
    auto_start: Option<bool>,
    auto_restart: Option<bool>,
    start_retries: Option<u32>,
}

impl <'a> SpawnCommandBuilder<'a> {
    pub fn new(configuration: &'a mut Vec<Vec<String>>) -> Self {
        Self {
            configuration,
            command: None,
            arguments: None,
            user: None,
            realtime_priority: None,
            auto_start: None,
            auto_restart: None,
            start_retries: None,
        }
    }

    pub fn with_command(&mut self, command: &str) -> &mut Self {
        self.command = Some(command.to_string());
        self
    }

    pub fn with_arguments(&mut self, arguments: Vec<&str>) -> &mut Self {
        self.arguments = Some(arguments.iter().map(|s| s.to_string()).collect());
        self
    }

    pub fn with_realtime_priority(&mut self, priority: u32) -> &mut Self {
        self.realtime_priority = Some(priority);
        self
    }

    pub fn with_user(&mut self, user: &str) -> &mut Self {
        self.user = Some(user.to_string());
        self
    }

    pub fn with_auto_start(&mut self, auto_start: bool) -> &mut Self {
        self.auto_start = Some(auto_start);
        self
    }

    pub fn with_auto_restart(&mut self, auto_restart: bool) -> &mut Self {
        self.auto_restart = Some(auto_restart);
        self
    }

    pub fn with_start_retries(&mut self, start_retries: u32) -> &mut Self {
        self.start_retries = Some(start_retries);
        self
    }

    pub fn build(&mut self) {
        if self.command.is_none() {
            panic!("Command is required");
        }
        let command = self.command.as_ref().unwrap();
        let process_name = PathBuf::from(command);
        let process_name = process_name.file_name().unwrap().to_str().unwrap();
        let command = match self.arguments.as_ref() {
            Some(value) => format!("{} {}", command, value.join(" ")),
            None => command.to_string(),
        };
        let command = match self.realtime_priority.as_ref() {
            Some(value) => format!("chrt {} {}", value, command),
            None => command,
        };
        let mut configuration = Vec::new();
        configuration.append(&mut vec![
            format!("[program:{}]", process_name),
            format!("command={}", command),
            format!("stdout_logfile=/tmp/{}.out.log", process_name),
            format!("stderr_logfile=/tmp/{}.err.log", process_name),
        ]);
        match self.user.as_ref() {
            Some(value) => configuration.push(format!("user={}", value.to_string()).to_string()),
            None => {}
        };
        match self.auto_start {
            Some(value) => configuration.push(format!("autostart={}", value.to_string()).to_string()),
            None => {}
        };
        match self.auto_restart {
            Some(value) => configuration.push(format!("autorestart={}", value.to_string()).to_string()),
            None => {}
        };
        match self.start_retries {
            Some(value) => configuration.push(format!("startretries={}", value.to_string()).to_string()),
            None => {}
        };
        self.configuration.push(configuration)
    }
}

impl_callback!(
    Fn => (),
    LifeCycleCallback,
    server : &mut ServerApi
);

pub struct Service {
    name: String,
    entrypoint: LifeCycleCallback,
    healthcheck: LifeCycleCallback,
}

impl Service {
    pub fn new(
        name: String,
        entrypoint: impl Fn(&mut ServerApi) + Sync + Send + 'static,
        healthcheck: impl Fn(&mut ServerApi) + Sync + Send + 'static,
    ) -> Self {
        Self {
            name,
            entrypoint: LifeCycleCallback::from(entrypoint),
            healthcheck: LifeCycleCallback::from(healthcheck),
        }
    }

    pub fn entrypoint(&mut self, server: &mut ServerApi) {
        self.entrypoint.call(server);
    }

    pub fn healthcheck(&mut self, server: &mut ServerApi) {
        self.healthcheck.call(server);
    }
}

impl Clone for Service {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            entrypoint: self.entrypoint.clone(),
            healthcheck: self.healthcheck.clone(),
        }
    }
}

pub struct ServerApi {
    name: String,
    tag: String,
    socket_id: Uuid,
    container_file_path: PathBuf,
    image_api: ImageApi,
    container_api: ContainerApi,
    container: Option<String>,
    configuration: Vec<Vec<String>>,
    entrypoint: Vec<Vec<String>>,
    healthcheck: Vec<Vec<String>>,
    post_start: Vec<Vec<String>>,
}

impl ServerApi {
    pub(self) fn new(
        name: String,
        container_file_path: PathBuf,
    ) -> Self {
        let environment = TEST_ENVIRONMENT.lock().unwrap();
        Self {
            name,
            tag: "latest".to_string(),
            socket_id: Uuid::new_v4(),
            container_file_path,
            image_api: environment.container_image_api.clone(),
            container_api: environment.container_api.clone(),
            container: None,
            configuration: Vec::new(),
            entrypoint: Vec::new(),
            healthcheck: Vec::new(),
            post_start: Vec::new(),
        }
    }

    fn socket_location(&self) -> PathBuf {
        Path::new("/run/pipewire-sockets").join(self.socket_id.to_string()).to_path_buf()
    }

    fn socket_name(&self) -> String {
        format!("{}", self.socket_id)
    }

    fn build(&self) {
        self.generate_configuration_file();
        self.generate_entrypoint_script();
        self.generate_healthcheck_script();
        self.image_api.build(
            &self.container_file_path,
            &self.name,
            &self.tag,
        );
    }

    fn create(&mut self) {
        let environment = TEST_ENVIRONMENT.lock().unwrap();
        let socket_location = self.socket_location();
        let socket_name = self.socket_name().to_string();
        let pulse_socket_location = socket_location.join("pulse");
        let mut create_options = CreateContainerOptionsBuilder::default();
        create_options
            .with_image(format!("{}:{}", self.name, self.tag))
            .with_environment(PIPEWIRE_RUNTIME_DIR_ENVIRONMENT_KEY, socket_location.to_str().unwrap())
            .with_environment(PIPEWIRE_CORE_ENVIRONMENT_KEY, socket_name.clone())
            .with_environment(PIPEWIRE_REMOTE_ENVIRONMENT_KEY, socket_name.clone())
            .with_environment(PULSE_RUNTIME_PATH_ENVIRONMENT_KEY, pulse_socket_location.to_str().unwrap())
            .with_environment("DISABLE_RTKIT", "y")
            .with_environment("DISPLAY", ":0.0")
            .with_volume("pipewire-sockets", socket_location.parent().unwrap().to_str().unwrap())
            .with_cpus(environment.container_cpu)
            .with_memory_swap(environment.container_memory_swap)
            .with_memory(environment.container_memory_swap);
        drop(environment);
        self.container = Some(self.container_api.create(&mut create_options));
    }

    fn start(&self) {
        self.container_api.start(self.container.as_ref().unwrap());
        self.container_api.wait_healthy(self.container.as_ref().unwrap());
    }

    fn stop(&self) {
        let mut options = StopContainerOptionsBuilder::default();
        self.container_api.stop(self.container.as_ref().unwrap(), &mut options);
    }

    fn restart(&self) {
        self.container_api.restart(self.container.as_ref().unwrap())
    }
    
    fn cleanup(&self) {
        let docker_api = Environment::create_docker_sync_api(&Duration::from_millis(100));
        let stop_options = StopContainerOptionsBuilder::default().build();
        docker_api.stop(&self.container.as_ref().unwrap(), Some(stop_options)).unwrap();
        let remove_options = RemoveContainerOptions {
            ..Default::default()
        };
        docker_api.remove(&self.container.as_ref().unwrap(), Some(remove_options)).unwrap();
    }

    fn spawn(&mut self, command: &str) -> SpawnCommandBuilder<'_> {
        let mut builder = SpawnCommandBuilder::new(&mut self.configuration);
        builder.with_command(command)
            .with_auto_start(true)
            .with_auto_restart(true);
        builder
    }

    fn spawn_wait_loop(&mut self) {
        self.entrypoint.push(vec![
            "supervisord".to_string(),
            "-c".to_string(),
            "/root/supervisor.conf".to_string(),
        ]);
    }

    fn create_folder(&mut self, path: &PathBuf) {
        self.entrypoint.push(vec![
            "mkdir".to_string(),
            "--parents".to_string(),
            path.to_str().unwrap().to_string(),
        ]);
    }

    fn create_socket_folder(&mut self) {
        self.entrypoint.push(vec![
            "mkdir".to_string(),
            "--parents".to_string(),
            format!("${{{}}}", PIPEWIRE_RUNTIME_DIR_ENVIRONMENT_KEY),
        ]);
    }

    fn remove_socket_folder(&mut self) {
        self.entrypoint.push(vec![
            "rm".to_string(),
            "--force".to_string(),
            "--recursive".to_string(),
            format!("${{{}}}", PIPEWIRE_RUNTIME_DIR_ENVIRONMENT_KEY),
        ]);
    }

    fn set_virtual_nodes_configuration(&mut self) {
        self.entrypoint.push(vec![
            "mkdir".to_string(),
            "--parents".to_string(),
            "/etc/pipewire/pipewire.conf.d/".to_string(),
        ]);
        self.entrypoint.push(vec![
            "cp".to_string(),
            "/root/virtual.nodes.conf".to_string(),
            "/etc/pipewire/pipewire.conf.d/virtual.nodes.conf".to_string(),
        ]);
    }

    fn set_default_nodes(&mut self) {
        self.post_start.push(vec![
            "echo".to_string(),
            "'wait for test-sink'".to_string()
        ]);
        self.post_start.push(vec![
            "pactl".to_string(),
            "set-default-sink".to_string(),
            "'test-sink'".to_string(),
        ]);
        self.post_start.push(vec![
            "wpctl".to_string(),
            "status".to_string(),
            "|".to_string(),
            "grep".to_string(),
            "--quiet".to_string(),
            "'test-sink'".to_string()
        ]);
        self.post_start.push(vec![
            "echo".to_string(),
            "'wait for test-source'".to_string()
        ]);
        self.post_start.push(vec![
            "pactl".to_string(),
            "set-default-source".to_string(),
            "'test-source'".to_string(),
        ]);
        self.post_start.push(vec![
            "wpctl".to_string(),
            "status".to_string(),
            "|".to_string(),
            "grep".to_string(),
            "--quiet".to_string(),
            "'test-source'".to_string()
        ]);
    }

    fn wait_for_pipewire(&mut self) {
        self.healthcheck.push(vec![
            "echo".to_string(),
            "'wait for pipewire'".to_string()
        ]);
        self.healthcheck.push(vec![
            "pw-cli".to_string(),
            "ls".to_string(),
            "0".to_string(),
            "|".to_string(),
            "grep".to_string(),
            "--quiet".to_string(),
            "'id 0, type PipeWire:Interface:Core/4'".to_string()
        ])
    }

    fn wait_for_wireplumber(&mut self) {
        self.healthcheck.push(vec![
            "echo".to_string(),
            "'wait for wireplumbler'".to_string()
        ]);
        self.healthcheck.push(vec![
            "wpctl".to_string(),
            "info".to_string(),
            "|".to_string(),
            "grep".to_string(),
            "--quiet".to_string(),
            "'WirePlumber'".to_string()
        ])
    }

    fn wait_for_pulse(&mut self) {
        self.healthcheck.push(vec![
            "echo".to_string(),
            "'wait for PipeWire Pulse'".to_string()
        ]);
        self.healthcheck.push(vec![
            "pactl".to_string(),
            "info".to_string(),
            "|".to_string(),
            "grep".to_string(),
            "--quiet".to_string(),
            "\"$PULSE_RUNTIME_PATH/native\"".to_string()
        ])
    }

    fn generate_configuration_file(&self) {
        let mut configuration = self.configuration.iter()
            .map(|e| e.join("\n"))
            .collect::<Vec<String>>();
        configuration.insert(0, "[supervisord]".to_string());
        configuration.insert(1, "nodaemon=true".to_string());
        configuration.insert(2, "logfile=/var/log/supervisor/supervisord.log".to_string());
        let configuration = configuration.join("\n");
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(CONTAINER_TMP_PATH.join("supervisor.conf"))
            .unwrap();
        file.write(configuration.as_bytes()).unwrap();
        file.flush().unwrap();
    }

    fn generate_entrypoint_script(&self) {
        let mut script = self.entrypoint.iter()
            .map(|command| command.join(" "))
            .collect::<Vec<String>>();
        script.insert(0, "#!/bin/bash".to_string());
        let script = script.join("\n");
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(CONTAINER_TMP_PATH.join("entrypoint.bash"))
            .unwrap();
        file.write(script.as_bytes()).unwrap();
        file.flush().unwrap();
    }

    fn generate_healthcheck_script(&self) {
        let mut script = self.healthcheck.iter()
            .map(|command| {
                format!("({}) || exit 1", command.join(" "))
            })
            .collect::<Vec<String>>();
        script.insert(0, "#!/bin/bash".to_string());
        script.insert(1, "set -e".to_string());
        let mut post_start_script = self.post_start.iter()
            .map(|command| {
                format!("({}) || exit 1", command.join(" "))
            })
            .collect::<Vec<String>>();
        script.append(&mut post_start_script);
        let script = script.join("\n");
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(CONTAINER_TMP_PATH.join("healthcheck.bash"))
            .unwrap();
        file.write(script.as_bytes()).unwrap();
        file.flush().unwrap();
    }
}

impl Drop for ServerApi {
    fn drop(&mut self) {
        if self.container.is_none() {
            return;
        }
        let mut stop_options = StopContainerOptionsBuilder::default();
        stop_options.with_wait(Duration::from_millis(0));
        self.container_api.stop(self.container.as_ref().unwrap(), &mut stop_options);
        self.container_api.remove(self.container.as_ref().unwrap());
    }
}

impl Clone for ServerApi {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            tag: self.tag.clone(),
            socket_id: self.socket_id.clone(),
            container_file_path: self.container_file_path.clone(),
            image_api: self.image_api.clone(),
            container_api: self.container_api.clone(),
            container: self.container.clone(),
            configuration: self.configuration.clone(),
            entrypoint: self.entrypoint.clone(),
            healthcheck: self.healthcheck.clone(),
            post_start: self.post_start.clone(),
        }
    }
}

pub struct ContainerizedServer {
    api: ServerApi,
    services: Vec<Service>,
    pre_entrypoint: Option<LifeCycleCallback>,
    post_start: Option<LifeCycleCallback>,
}

impl ContainerizedServer {
    pub(self) fn new(
        name: String,
        container_file_path: PathBuf,
        services: Vec<Service>,
        pre_entrypoint: Option<LifeCycleCallback>,
        post_start: Option<LifeCycleCallback>,
    ) -> Self {
        Self {
            api: ServerApi::new(name, container_file_path),
            services,
            pre_entrypoint,
            post_start,
        }
    }

    pub fn build(&mut self) {
        self.api.create_socket_folder();
        match &self.pre_entrypoint {
            Some(value) => value.call(&mut self.api),
            None => {}
        }
        for service in &mut self.services {
            service.entrypoint.call(&mut self.api);
            service.healthcheck.call(&mut self.api);
        }
        match &self.post_start {
            Some(value) => value.call(&mut self.api),
            None => {}
        }
        self.api.spawn_wait_loop();
        self.api.remove_socket_folder();
        self.api.build();
    }

    pub fn create(&mut self) {
        self.api.create();
    }

    pub fn start(&mut self) {
        self.api.start()
    }

    pub fn stop(&mut self) {
        self.api.stop()
    }

    pub fn restart(&mut self) {
        self.api.restart();
    }

    pub fn set_socket_env_vars(&self) {
        std::env::set_var(PIPEWIRE_RUNTIME_DIR_ENVIRONMENT_KEY, self.api.socket_location());
        std::env::set_var(PIPEWIRE_REMOTE_ENVIRONMENT_KEY, self.api.socket_name());
    }
    
    pub(self) fn cleanup(&self) {
        self.api.cleanup();        
    }
}

impl Clone for ContainerizedServer {
    fn clone(&self) -> Self {
        Self {
            api: self.api.clone(),
            services: self.services.clone(),
            pre_entrypoint: self.pre_entrypoint.clone(),
            post_start: self.post_start.clone(),
        }
    }
}

impl Drop for ContainerizedServer {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct LocalServer {}

pub enum Server {
    Containerized(ContainerizedServer),
    Local
}

impl Server {
    pub fn start(&mut self) {
        match self {
            Server::Containerized(value) => {
                value.start();
            }
            Server::Local => {}
        }
    }

    pub fn clone(&self) -> Self {
        match self {
            Server::Containerized(value) => Server::Containerized(value.clone()),
            Server::Local => Server::Local
        }
    }
    
    pub fn cleanup(&self) {
        match self {
            Server::Containerized(value) => value.cleanup(),
            Server::Local => {}
        }
    }
}

#[fixture]
pub fn server_with_default_configuration() -> Arc<Server> {
    let services = vec![
        PIPEWIRE_SERVICE.clone(),
        WIREPLUMBER_SERVICE.clone(),
        PULSE_SERVICE.clone(),
    ];
    let mut server = ContainerizedServer::new(
        "pipewire-default".to_string(),
        CONTAINER_FILE_PATH.clone(),
        services,
        Some(LifeCycleCallback::from(|server: &mut ServerApi| {
            server.set_virtual_nodes_configuration();
        })),
        Some(LifeCycleCallback::from(|server: &mut ServerApi| {
            server.set_default_nodes();
        })),
    );
    let environment = TEST_ENVIRONMENT.lock().unwrap();
    let test_target = environment.test_target.clone();
    drop(environment);
    match test_target {
        TestTarget::Local => Arc::new(Server::Local),
        TestTarget::Container => {
            server.build();
            server.create();
            server.start();
            server.set_socket_env_vars();
            Arc::new(Server::Containerized(server))
        }
    }
}

#[fixture]
pub fn server_without_session_manager() -> Arc<Server> {
    let services = vec![
        PIPEWIRE_SERVICE.clone(),
    ];
    let mut server = ContainerizedServer::new(
        "pipewire-without-session-manager".to_string(),
        CONTAINER_FILE_PATH.clone(),
        services,
        None,
        None,
    );
    server.build();
    server.create();
    server.start();
    server.set_socket_env_vars();
    Arc::new(Server::Containerized(server))
}

#[fixture]
pub fn server_without_node() -> Arc<Server> {
    let services = vec![
        PIPEWIRE_SERVICE.clone(),
        WIREPLUMBER_SERVICE.clone(),
    ];
    let mut server = ContainerizedServer::new(
        "pipewire-without-node".to_string(),
        CONTAINER_FILE_PATH.clone(),
        services,
        None,
        None,
    );
    server.build();
    server.create();
    server.start();
    server.set_socket_env_vars();
    Arc::new(Server::Containerized(server))
}