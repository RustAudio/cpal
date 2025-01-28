use bollard::container::{RemoveContainerOptions, StopContainerOptions};
use bollard::errors::Error;
use bollard::errors::Error::{DockerResponseServerError, RequestTimeoutError};
use bollard::{ClientVersion};
use bytes::{Buf, Bytes};
use http::header::CONTENT_TYPE;
use http::{Method, Request, Response, StatusCode, Version};
use pipewire_common::utils::Backoff;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::ffi::OsStr;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::{fmt, io};
use std::path::Path;
use http::request::Builder;
use ureq::{Agent, Body, RequestBuilder, SendBody};
use ureq::middleware::MiddlewareNext;
use ureq::typestate::{WithBody, WithoutBody};
use url::Url;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct DockerServerErrorMessage {
    message: String,
}

pub trait Connector {
    fn execute(&self, request: &mut Request<Bytes>) -> http::Result<Response<Bytes>>;
}

pub struct HttpConnector {
    agent: Agent
}

impl HttpConnector {
    pub fn new() -> Self {
        let config = Agent::config_builder()
            .build();
        let agent = config.into();
        Self {
            agent,
        }
    }
    
    fn request_builder(&self, request: &mut Request<Bytes>) -> ureq::RequestBuilder<WithBody> {
        if request.method() == http::Method::GET {
            self.agent.get(request.uri())
                .force_send_body()
        }
        else if request.method() == http::Method::POST { 
            self.agent.post(request.uri())
        }
        else if request.method() == http::Method::PUT { 
            self.agent.put(request.uri())
        }
        else if request.method() == http::Method::PATCH { 
            self.agent.patch(request.uri())
        }
        else if request.method() == http::Method::DELETE {  
            self.agent.delete(request.uri()).force_send_body()
        }
        else if request.method() == http::Method::HEAD { 
            self.agent.head(request.uri()).force_send_body()
        }
        else if request.method() == http::Method::OPTIONS { 
            self.agent.options(request.uri()).force_send_body()
        }
        else {
            panic!("Not supported HTTP method")
        }
    }
}

impl Connector for HttpConnector
{
    fn execute(&self, mut request: &mut Request<Bytes>) -> http::Result<Response<Bytes>> {
        let mut builder = self.request_builder(request)
            .version(Version::HTTP_11)
            .uri(request.uri());
        for (key, value) in request.headers() {
            builder = builder.header(key.as_str(), value.to_str().unwrap());
        }
        let mut data = Vec::new();
        let mut bytes = request.body_mut().reader();
        io::copy(&mut bytes, &mut data).unwrap();
        let body = Body::builder()
            .data(data);
        let mut response = builder.send(body).unwrap();
        let mut builder = http::Response::builder()
            .status(response.status())
            .version(response.version());
        for (key, value) in response.headers().iter() {
            builder = builder.header(key.as_str(), value.to_str().unwrap());
        }
        let mut data = Vec::new();
        let mut reader = response.body_mut().as_reader();
        io::copy(&mut reader, &mut data).unwrap();
        let bytes = Bytes::from(data);
        builder.body(bytes)
    }
}

pub struct UnixConnector {

}

impl UnixConnector {
    pub fn new() -> Self {
        Self {

        }
    }
}

impl Connector for UnixConnector {
    fn execute(&self, request: &mut Request<Bytes>) -> http::Result<Response<Bytes>> {
        todo!()
    }
}

pub struct Client<C>
    where C: Connector
{
    connector: C,
}

impl <C> Client<C>
where
    C: Connector
{
    pub fn new(connector: C) -> Self {
        Self {
            connector,
        }
    }

    pub fn execute(&self, request: &mut Request<Bytes>) -> http::Result<Response<Bytes>> {
        self.connector.execute(request)
    }
}

pub(crate) enum Transport {
    Http {
        client: Client<HttpConnector>,
    },
    Unix {
        client: Client<UnixConnector>,
    },
}

impl fmt::Debug for Transport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Transport::Http { .. } => write!(f, "HTTP"),
            Transport::Unix { .. } => write!(f, "Unix"),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum ClientType {
    Unix,
    Http,
}

#[derive(Debug)]
pub struct Uri<'a> {
    encoded: Cow<'a, str>,
}

impl<'a> Uri<'a> {
    pub(crate) fn parse<O>(
        socket: &'a str,
        client_type: &ClientType,
        path: &'a str,
        query: Option<O>,
        client_version: &ClientVersion,
    ) -> Result<Self, Error>
    where
        O: serde::ser::Serialize,
    {
        let host_str = format!(
            "{}://{}/v{}.{}{}",
            Uri::socket_scheme(client_type),
            Uri::socket_host(socket, client_type),
            client_version.major_version,
            client_version.minor_version,
            path
        );
        let mut url = Url::parse(host_str.as_ref())?;
        url = url.join(path)?;

        if let Some(pairs) = query {
            let qs = serde_urlencoded::to_string(pairs)?;
            url.set_query(Some(&qs));
        }
        Ok(Uri {
            encoded: Cow::Owned(url.as_str().to_owned()),
        })
    }

    fn socket_host<P>(socket: P, client_type: &ClientType) -> String
    where
        P: AsRef<OsStr>,
    {
        match client_type {
            ClientType::Http => socket.as_ref().to_string_lossy().into_owned(),
            ClientType::Unix => hex::encode(socket.as_ref().to_string_lossy().as_bytes()),
        }
    }

    fn socket_scheme(client_type: &'a ClientType) -> &'a str {
        match client_type {
            ClientType::Http => "http",
            ClientType::Unix => "unix",
        }
    }
}

pub struct SyncContainerApi {
    pub(crate) transport: Arc<Transport>,
    pub(crate) client_type: ClientType,
    pub(crate) client_addr: String,
    pub(crate) client_timeout: u64,
    pub(crate) version: Arc<(AtomicUsize, AtomicUsize)>,
}

impl SyncContainerApi {
    pub fn connect_with_http(
        addr: &str,
        timeout: u64,
        client_version: &ClientVersion,
    ) -> Result<Self, Error> {
        let client_addr = addr.replacen("tcp://", "", 1).replacen("http://", "", 1);
        let http_connector = HttpConnector::new();
        let client = Client::new(http_connector);
        let transport = Transport::Http { client };
        let docker = Self {
            transport: Arc::new(transport),
            client_type: ClientType::Http,
            client_addr,
            client_timeout: timeout,
            version: Arc::new((
                AtomicUsize::new(client_version.major_version),
                AtomicUsize::new(client_version.minor_version),
            )),
        };
        Ok(docker)
    }

    pub fn connect_with_socket(
        path: &str,
        timeout: u64,
        client_version: &ClientVersion,
    ) -> Result<Self, Error> {
        let clean_path = path.trim_start_matches("unix://");
        if !std::path::Path::new(clean_path).exists() {
            return Err(Error::SocketNotFoundError(clean_path.to_string()));
        }
        let docker = Self::connect_with_unix(path, timeout, client_version)?;
        Ok(docker)
    }

    pub fn connect_with_unix(
        path: &str,
        timeout: u64,
        client_version: &ClientVersion,
    ) -> Result<Self, Error> {
        let client_addr = path.replacen("unix://", "", 1);
        if !Path::new(&client_addr).exists() {
            return Err(Error::SocketNotFoundError(client_addr));
        }
        let unix_connector = UnixConnector::new();
        let client = Client::new(unix_connector);
        let transport = Transport::Unix { client };
        let docker = Self {
            transport: Arc::new(transport),
            client_type: ClientType::Unix,
            client_addr,
            client_timeout: timeout,
            version: Arc::new((
                AtomicUsize::new(client_version.major_version),
                AtomicUsize::new(client_version.minor_version),
            )),
        };
        Ok(docker)
    }

    pub fn client_version(&self) -> ClientVersion {
        self.version.as_ref().into()
    }

    pub fn stop(
        &self,
        container_name: &str,
        options: Option<StopContainerOptions>,
    ) -> Result<(), Error> {
        let url = format!("/containers/{container_name}/stop");

        let req = self.build_request(
            &url,
            Builder::new().method(Method::POST),
            options,
            Ok(Bytes::new()),
        );
        self.process_request(req)?;
        Ok(())
    }

    pub fn remove(
        &self,
        container_name: &str,
        options: Option<RemoveContainerOptions>,
    ) -> Result<(), Error> {
        let url = format!("/containers/{container_name}");
        let req = self.build_request(
            &url,
            Builder::new().method(Method::DELETE),
            options,
            Ok(Bytes::new()),
        );
        self.process_request(req)?;
        Ok(())
    }

    pub(crate) fn build_request<O>(
        &self,
        path: &str,
        builder: Builder,
        query: Option<O>,
        payload: Result<Bytes, Error>,
    ) -> Result<Request<Bytes>, Error>
    where
        O: Serialize,
    {
        let uri = Uri::parse(
            &self.client_addr,
            &self.client_type,
            path,
            query,
            &self.client_version(),
        )?;
        Ok(builder
            .uri(uri.encoded.to_string())
            .header(CONTENT_TYPE, "application/json")
            .body(payload?)?)
    }

    pub(crate) fn process_request(
        &self,
        request: Result<Request<Bytes>, Error>,
    ) -> Result<Response<Bytes>, Error> {
        let transport = self.transport.clone();
        let timeout = self.client_timeout;

        let mut request = request?;
        let response = Self::execute_request(transport, &mut request, timeout)?;

        let status = response.status();
        match status {
            // Status code 200 - 299 or 304
            s if s.is_success() || s == StatusCode::NOT_MODIFIED => Ok(response),

            StatusCode::SWITCHING_PROTOCOLS => Ok(response),

            // All other status codes
            _ => {
                let contents = Self::decode_into_string(response)?;

                let mut message = String::new();
                if !contents.is_empty() {
                    message = serde_json::from_str::<DockerServerErrorMessage>(&contents)
                        .map(|msg| msg.message)
                        .or_else(|e| {
                            if e.is_data() || e.is_syntax() {
                                Ok(contents)
                            } else {
                                Err(e)
                            }
                        })?;
                }
                Err(DockerResponseServerError {
                    status_code: status.as_u16(),
                    message,
                })
            }
        }
    }

    fn execute_request(
        transport: Arc<Transport>,
        request: &mut Request<Bytes>,
        timeout: u64,
    ) -> Result<Response<Bytes>, Error> {
        let operation = || {
            let request = match *transport {
                Transport::Http { ref client } => client.execute(request),
                Transport::Unix { ref client } => client.execute(request),
            };
            let request = request.map_err(Error::from);
            match request {
                Ok(value) => Ok(value),
                Err(value) => Err(value),
            }
        };
        let mut backoff = Backoff::constant((timeout * 1000) as u128);
        backoff.retry(operation).map_err(|_| RequestTimeoutError)
    }

    fn decode_into_string(response: Response<Bytes>) -> Result<String, Error> {
        let body = response.into_body();
        Ok(String::from_utf8_lossy(&body).to_string())
    }
}