use pipewire_common::utils::Backoff;
use bollard::container::{ListContainersOptions, LogOutput, RestartContainerOptions, UploadToContainerOptions};
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use bollard::image::{BuildImageOptions, BuilderVersion};
use bollard::{Docker};
use bytes::Bytes;
use futures::StreamExt;
use std::collections::HashMap;
use std::{fs, io};
use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use bollard::errors::Error;
use bollard::models::{ContainerInspectResponse, ContainerState, ContainerSummary, Health, HealthStatusEnum, ImageInspect};
use sha2::{Digest, Sha256};
use tar::{Builder, Header};
use tokio::runtime::Runtime;
use uuid::Uuid;
use crate::containers::options::{CreateContainerOptionsBuilder, StopContainerOptionsBuilder};
use crate::environment::TEST_ENVIRONMENT;
use crate::HexSlice;

pub(crate) static CONTAINER_PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("pipewire-test-utils")
        .join(".containers")
        .as_path()
        .to_path_buf()
});

pub(crate) static CONTAINER_TMP_PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    let path = CONTAINER_PATH
        .join(".tmp")
        .as_path()
        .to_path_buf();
    fs::create_dir_all(&path).unwrap();
    path
});

pub(crate) static CONTAINER_DIGESTS_FILE_PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    CONTAINER_PATH
        .join(".digests")
        .as_path()
        .to_path_buf()
});

pub struct ImageRegistry {
    images: HashMap<String, String>,
}

impl ImageRegistry {
    pub fn new() -> Self {
        let file = match fs::read_to_string(&*CONTAINER_DIGESTS_FILE_PATH) {
            Ok(value) => value,
            Err(_) => return Self {
                images: HashMap::new(),
            }
        };
        let images = file.lines()
            .into_iter()
            .map(|line| {
                let line_parts = line.split("=").collect::<Vec<&str>>();
                let image_name = line_parts[0];
                let container_file_digest = line_parts[1];
                (image_name.to_string(), container_file_digest.to_string().to_string())
            })
            .collect::<HashMap<_, _>>();
        Self {
            images,
        }
    }

    pub fn push(&mut self, image_name: String, container_file_digest: String) {
        if self.images.contains_key(&image_name) {
            *self.images.get_mut(&image_name).unwrap() = container_file_digest
        }
        else {
            self.images.insert(image_name, container_file_digest);
        }
    }
    
    pub fn is_build_needed(&self, image_name: &String, digest: &String) -> bool {
        self.images.get(image_name).map_or(true, |entry| {
            *entry != *digest
        })
    }

    pub(crate) fn cleanup(&self) {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&*CONTAINER_DIGESTS_FILE_PATH)
            .unwrap();
        for (image_name, container_file_digest) in self.images.iter() {
            unsafe {
                let format = CString::new("Registering image digests to further process: %s\n").unwrap();
                libc::printf(format.as_ptr() as *const i8, CString::new(image_name.clone()).unwrap()); 
            }
            // println!("Registering image digests to further process: {}", image_name);
            writeln!(
                file,
                "{}={}",
                image_name,
                container_file_digest
            ).unwrap();
        }
    }
}

pub struct ContainerRegistry {
    api: ContainerApi,
    containers: Vec<String>
}

impl ContainerRegistry {
    pub fn new(api: ContainerApi) -> Self {
        let registry = Self {
            api: api.clone(),
            containers: Vec::new(),
        };
        registry.clean();
        registry
    }

    pub(crate) fn clean(&self) {
        let containers = self.api.get_all().unwrap();
        for container in containers {
            let container_id = container.id.unwrap();
            let inspect_result = match self.api.inspect(&container_id) {
                Ok(value) => value,
                Err(_) => continue
            };
            if let Some(state) = inspect_result.state {
                self.api.clean(&container_id.to_string(), &state);
            }
        }
    }
}

struct ImageContext {
}

impl ImageContext {
    fn create(container_file_path: &PathBuf) -> Result<(Bytes, String), Error> {
        let excluded_filename = vec![
            ".digests",
        ];
        let context_path = container_file_path.parent().unwrap();
        // Hasher is used for computing all context files hashes.
        // In that way we can determine later with we build the image or not.
        // This is better that just computing context archive hash which include data and metadata
        // that can change regarding if context files had not changed in times.
        let mut hasher = Sha256::new();
        let mut archive = tar::Builder::new(Vec::new());
        Self::read_directory(
            &mut archive,
            &mut hasher,
            context_path,
            context_path,
            Some(&excluded_filename)
        )?;
        let uncompressed = archive.into_inner()?;
        let mut compressed = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        compressed.write_all(&uncompressed)?;
        let compressed = compressed.finish()?;
        let data = Bytes::from(compressed);
        let hash_bytes = hasher.finalize().to_vec();
        let digest = HexSlice(hash_bytes.as_slice());
        let digest = format!("sha256:{}", digest);
        Ok((data, digest))
    }

    fn read_directory(
        archive: &mut Builder<Vec<u8>>,
        hasher: &mut impl Write,
        root: &Path,
        directory: &Path,
        excluded_filenames: Option<&Vec<&str>>
    ) -> io::Result<()> {
        if directory.is_dir() {
            for entry in fs::read_dir(directory)? {
                let entry = entry?;
                let path = entry.path();
                let filename = path.file_name().unwrap().to_str().unwrap();
                if path.is_dir() {
                    Self::read_directory(archive, hasher, root, &path, excluded_filenames)?;
                }
                else if path.is_file() {
                    if excluded_filenames.as_ref().unwrap().contains(&filename) {
                        continue;
                    }
                    let mut file = File::open(&path)?;
                    io::copy(&mut file, hasher)?;
                    file.seek(SeekFrom::Start(0))?;
                    let mut header = Header::new_gnu();
                    let metadata = file.metadata()?;
                    header.set_path(path.strip_prefix(root).unwrap())?;
                    header.set_size(metadata.len());
                    header.set_mode(metadata.permissions().mode());
                    header.set_mtime(metadata.modified()?.elapsed().unwrap().as_secs());
                    header.set_cksum();
                    archive.append(&header, &mut file)?;
                }
            }
        }
        Ok(())
    }
}

pub struct ImageApi {
    runtime: Arc<Runtime>,
    api: Arc<Docker>
}

impl ImageApi {
    pub fn new(runtime: Arc<Runtime>, api: Arc<Docker>) -> Self {
        Self {
            runtime,
            api,
        }
    }

    pub fn inspect(&self, image_name: &String) -> Result<ImageInspect, Error> {
        let result = self.api.inspect_image(image_name.as_str());
        self.runtime.block_on(result)
    }

    pub fn build(
        &self,
        container_file_path: &PathBuf,
        image_name: &String,
        image_tag: &String
    ) {
        let tag = format!("{}:{}", image_name, image_tag);
        let options = BuildImageOptions {
            dockerfile: container_file_path.file_name().unwrap().to_str().unwrap(),
            t: tag.as_str(),
            session: Some(Uuid::new_v4().to_string()),
            version: BuilderVersion::BuilderBuildKit,
            ..Default::default()
        };
        let (context, context_digest) = ImageContext::create(&container_file_path).unwrap();
        let mut environment = TEST_ENVIRONMENT.lock().unwrap();
        println!("Container image digest: {}", context_digest);
        if environment.container_image_registry.is_build_needed(&image_name, &context_digest) == false {
            println!("Skip build container image: {}", tag);
            return;
        }
        println!("Build container image: {}", tag);
        let mut stream = self.api.build_image(options, None, Some(context));
        while let Some(message) = self.runtime.block_on(stream.next()) {
            match message {
                Ok(message) => {
                    if let Some(stream) = message.stream {
                        if cfg!(debug_assertions) {
                            print!("{}", stream)
                        }
                    }
                    else if let Some(error) = message.error {
                        panic!("{}", error);
                    }
                }
                Err(value) => {
                    panic!("Error during image build: {:?}", value);
                }
            }
        };
        environment.container_image_registry.push(
            image_name.clone(),
            context_digest.clone()
        );
    }
}

impl Clone for ImageApi {
    fn clone(&self) -> Self {
        Self {
            runtime: self.runtime.clone(),
            api: self.api.clone(),
        }
    }
}

pub struct ContainerApi {
    runtime: Arc<Runtime>,
    api: Arc<Docker>
}

impl ContainerApi {
    pub fn new(runtime: Arc<Runtime>, api: Arc<Docker>) -> Self {
        Self {
            runtime,
            api,
        }
    }

    pub(self) fn get_all(&self) -> Result<Vec<ContainerSummary>, Error>{
        let mut filter = HashMap::new();
        filter.insert("label", vec!["test.container=true"]);
        let options = ListContainersOptions {
            all: true,
            filters: filter,
            ..Default::default()
        };
        let call = self.api.list_containers(Some(options));
        self.runtime.block_on(call)
    }
    
    pub(self) fn clean(&self, id: &String, state: &ContainerState) {
        println!("Clean container with id {}", id);
        if state.running.unwrap() {
            let stop_options = StopContainerOptionsBuilder::default().build();
            let call = self.api.stop_container(id, Some(stop_options));
            self.runtime.block_on(call).unwrap();
        }
        let call = self.api.remove_container(id, None);
        self.runtime.block_on(call).unwrap();
    }

    pub fn create(&self, options: &mut CreateContainerOptionsBuilder) -> String {
        let options = options
            .with_label("test.container", true.to_string())
            .build();
        println!("Create container with image {}", options.image.as_ref().unwrap());
        let call = self.api.create_container::<String, String>(None, options);
        let result = self.runtime.block_on(call).unwrap();
        result.id
    }

    pub fn start(&self, id: &String) {
        println!("Start container with id {}", id);
        let call = self.api.start_container::<String>(id, None);
        self.runtime.block_on(call).unwrap();
    }

    pub fn stop(&self, id: &String, options: &mut StopContainerOptionsBuilder) {
        println!("Stop container with id {}", id);
        let options = options.build();
        let call = self.api.stop_container(id, Some(options));
        self.runtime.block_on(call).unwrap();
    }
    
    pub fn restart(&self, id: &String) {
        println!("Restart container with id {}", id);
        let options = RestartContainerOptions {
            t: 0,
        };
        let call = self.api.restart_container(id, Some(options));
        self.runtime.block_on(call).unwrap();
    }

    pub fn remove(&self, id: &String) {
        println!("Remove container with id {}", id);
        let call = self.api.remove_container(id, None);
        self.runtime.block_on(call).unwrap();
    }

    pub fn inspect(&self, id: &String) -> Result<ContainerInspectResponse, pipewire_common::error::Error> {
        let call = self.api.inspect_container(id, None);
        self.runtime.block_on(call).map_err(|error| {
            pipewire_common::error::Error {
                description: error.to_string(),
            }
        })
    }
    
    pub fn upload(&self, id: &String, path: &str, archive: Bytes) {
        let options = UploadToContainerOptions {
            path: path.to_string(),
            no_overwrite_dir_non_dir: true.to_string(),
        };
        let call = self.api.upload_to_container(id, Some(options), archive);
        self.runtime.block_on(call).unwrap();
    }

    pub fn wait_healthy(&self, id: &String) {
        println!("Wait container with id {} to be healthy", id);
        let operation = || {
            let response = self.inspect(id);
            match response {
                Ok(value) => {
                    let state = value.state.unwrap();
                    let health = state.health.unwrap();
                    match health {
                        Health { status, .. } => {
                            match status.unwrap() {
                                HealthStatusEnum::HEALTHY => Ok(()),
                                _ => Err(pipewire_common::error::Error {
                                    description: "Container not yet healthy".to_string(),
                                })
                            }
                        }
                    }
                }
                Err(value) => Err(pipewire_common::error::Error {
                    description: format!("Container {} not ready: {}", id, value),
                })
            }
        };
        let mut backoff = Backoff::default();
        backoff.retry(operation).unwrap()
    }
    
    pub fn exec(
        &self,
        id: &String,
        command: Vec<&str>,
        detach: bool,
        expected_exit_code: u32,
    ) -> Result<Vec<String>, pipewire_common::error::Error> {
        let create_exec_options = CreateExecOptions {
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            tty: Some(true),
            cmd: Some(command),
            ..Default::default()
        };
        let call = self.api.create_exec(id.as_str(), create_exec_options);
        let create_exec_result = self.runtime.block_on(call).unwrap();
        let exec_id = create_exec_result.id;
        let start_exec_options = StartExecOptions {
            detach,
            tty: true,
            ..Default::default()
        };
        let call = self.api.start_exec(exec_id.as_str(), Some(start_exec_options));
        let start_exec_result = self.runtime.block_on(call).unwrap();
        let mut output_result: Vec<String> = Vec::new();
        if let StartExecResults::Attached { mut output, .. } = start_exec_result {
            while let Some(Ok(message)) = self.runtime.block_on(output.next()) {
                match message {
                    LogOutput::StdOut { message } => {
                        output_result.push(
                            String::from_utf8(message.to_vec()).unwrap()
                        )
                    }
                    LogOutput::StdErr { message } => {
                        eprint!("{}", String::from_utf8(message.to_vec()).unwrap())
                    }
                    LogOutput::Console { message } => {
                        output_result.push(
                            String::from_utf8(message.to_vec()).unwrap()
                        )
                    }
                    _ => {}
                }
            }
            let call = self.api.inspect_exec(exec_id.as_str());
            let exec_inspect_result = self.runtime.block_on(call).unwrap();
            let exit_code = exec_inspect_result.exit_code.unwrap();
            if exit_code != expected_exit_code as i64 {
                return Err(pipewire_common::error::Error {
                    description: format!("Unexpected exit code: {exit_code}"),
                });
            }
            let output_result = output_result.iter()
                .flat_map(move |output| {
                    output.split('\n')
                        .map(move |line| line.trim().to_string())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            Ok(output_result)
        } else {
            Ok(output_result)
        }
    }

    pub fn top(&self, id: &String) -> HashMap<String, String> {
        let call = self.api.top_processes::<&str>(id, None);
        let result = self.runtime.block_on(call).unwrap();
        let titles = result.titles.unwrap();
        let pid_column_index = titles.iter().position(move |title| *title == "PID").unwrap();
        let cmd_column_index = titles.iter().position(move |title| *title == "CMD").unwrap();
        let processes = result.processes.unwrap().iter()
            .map(|process| {
                let pid = process.get(pid_column_index).unwrap();
                let cmd = process.get(cmd_column_index).unwrap();
                (cmd.clone(), pid.clone())
            })
            .collect::<HashMap<_, _>>();
        processes
    }

    pub fn wait_for_pid(&self, id: &String, process_name: &str) -> u32 {
        let operation = || {
            let result = self.top(id);
            let pid = result.iter()
                .map(move |(cmd, pid)| {
                    let cmd = cmd.split(" ").collect::<Vec<_>>();
                    let cmd = *cmd.first().unwrap();
                    (cmd, pid.clone())
                })
                .filter(move |(cmd, _)| **cmd == *process_name)
                .map(|(_, pid)| pid.parse::<u32>().unwrap())
                .collect::<Vec<u32>>();
            match pid.first() {
                Some(value) => Ok(value.clone()),
                None => Err(pipewire_common::error::Error {
                    description: "Process not yet spawned".to_string(),
                })
            }
        };
        let mut backoff = Backoff::default();
        backoff.retry(operation).unwrap()
    }

    fn wait_for_file_type(&self, id: &String, file_type: &str, path: &PathBuf) {
        let file_type_argument = match file_type {
            "file" => "-f",
            "socket" => "-S",
            _ => panic!("Cannot determine file type"),
        };
        let operation = || {
            self.exec(
                id,
                vec![
                    "test", file_type_argument, path.to_str().unwrap()
                ],
                false,
                0
            )?;
            Ok::<(), pipewire_common::error::Error>(())
        };
        let mut backoff = Backoff::default();
        backoff.retry(operation).unwrap()
    }

    pub fn wait_for_file(&self, id: &String, path: &PathBuf) {
        self.wait_for_file_type(id, "file", path);
    }

    pub fn wait_for_socket_file(&self, id: &String, path: &PathBuf) {
        self.wait_for_file_type(id, "socket", path);
    }

    pub fn wait_for_socket_listening(&self, id: &String, path: &PathBuf) {
        let operation = || {
            self.exec(
                id,
                vec![
                    "socat", "-u", "OPEN:/dev/null",
                    format!("UNIX-CONNECT:{}", path.to_str().unwrap()).as_str()
                ],
                false,
                0
            )?;
            Ok::<(), pipewire_common::error::Error>(())
        };
        let mut backoff = Backoff::default();
        backoff.retry(operation).unwrap()
    }

    pub fn wait_for_command_output(&self, id: &String, command: Vec<&str>, expected_output: &str) {
        let operation = || {
            let command_output = self.exec(
                id,
                command.clone(),
                false,
                0
            )?;
            return if command_output.iter().any(|output| {
                let output = output.trim();
                output == expected_output
            }) {
                Ok(())
            } else {
                Err(pipewire_common::error::Error {
                    description: format!("Unexpected output {}", expected_output)
                })
            };
        };
        let mut backoff = Backoff::default();
        backoff.retry(operation).unwrap()
    }
}

impl Clone for ContainerApi {
    fn clone(&self) -> Self {
        Self {
            runtime: self.runtime.clone(),
            api: self.api.clone(),
        }
    }
}