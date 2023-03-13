use bollard::container::ListContainersOptions;
use bollard::image::ListImagesOptions;
use bollard::{Docker, API_DEFAULT_VERSION};
use dns_lookup::lookup_addr;
use network_interface::Addr;
use network_interface::NetworkInterface;
use network_interface::NetworkInterfaceConfig;
use network_interface::V4IfAddr;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::RandomState;
use std::collections::HashMap;
use std::fs::canonicalize;
use std::net::UdpSocket;
use std::process::Command;
use std::str;
use std::time::Duration;
use tauri::command;

#[derive(Debug, Deserialize)]
pub struct RequestBody {
    id: i32,
    name: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
enum DockerConnectionStrategy {
    LOCAL,
    REMOTE,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DockerConfig {
    strategy: DockerConnectionStrategy,

    #[serde(default)]
    docker_addr: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DockerInfo {
    memory: String,
    version: String,
}
#[derive(Debug, Deserialize, Serialize)]
pub struct Endpoint {
    #[serde(default)]
    docker_addr: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Beacon {
    #[serde(default)]
    url: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BeaconSignal {
    #[serde(default)]
    url: String,
    bind: String,
    broadcast: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AdvertiseOk {
    ok: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InitializeRequest {
    dirpath: String,
    yaml: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InitializeAnswer {
    ok: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpRequest {
    dirpath: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpAnswer {
    ok: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct StopRequest {
    dirpath: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct StopAnswer {
    ok: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Container {
    id: Option<String>,
    names: Option<Vec<String>>,
    image: Option<String>,
    labels: Option<HashMap<String, String, RandomState>>,
    status: Option<String>,
    state: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ContainerQuery {
    containers: Vec<Container>,
}

#[command]
pub fn hello_world_test(event: String) -> Option<String> {
    let stdout = hello_world(event);
    Some(stdout)
}

#[command]
pub async fn test_docker(event: String) -> Option<String> {
    let x: DockerConfig = serde_json::from_str(event.as_str()).unwrap();

    let docker;

    match x.strategy {
        DockerConnectionStrategy::LOCAL => {
            docker = Docker::connect_with_local_defaults().unwrap();
        }
        DockerConnectionStrategy::REMOTE => {
            docker =
                Docker::connect_with_http(x.docker_addr.as_str(), 4, API_DEFAULT_VERSION).unwrap();
        }
    }

    let version = docker.version().await.unwrap();
    let system_info = docker.info().await.unwrap();

    let info = DockerInfo {
        memory: system_info.mem_total.unwrap().to_string(),
        version: version.api_version.unwrap(),
    };

    Some(serde_json::to_string(&info).unwrap())
}

#[command]
pub async fn advertise_endpoint(signals: Vec<BeaconSignal>) -> Option<String> {
    for beacon in signals {
        let serde_json = serde_json::to_string(&Beacon { url: beacon.url }).unwrap();

        let bind_ip = beacon.bind;
        let bind_addr = format!("{}:0", bind_ip.to_string());
        let broadcast_ip = beacon.broadcast.unwrap();
        let broadcast_addr = format!("{}:45678", broadcast_ip.to_string());

        let socket: UdpSocket = UdpSocket::bind(bind_addr).unwrap();
        socket.set_read_timeout(Some(Duration::new(5, 0))).unwrap();
        socket.set_broadcast(true).unwrap();

        println!("Broadcast: {:?}", broadcast_addr);
        println!("Timeout: {:?}", socket.read_timeout());

        let call: Vec<u8> = format!("beacon-fakts{}", serde_json).as_bytes().to_vec();
        println!("Sending call, {} bytes", call.len());
        socket.send_to(&call, broadcast_addr).unwrap();
    }

    Some("ok".to_string())
}

#[command]
pub async fn nana_test(deployment: String) -> Option<ContainerQuery> {
    let docker = Docker::connect_with_local_defaults().expect("Failed to connect to docker");

    let version = docker.version().await.unwrap();

    let string;
    let searchString = {
        string = format!("arkitekt.{}.service", deployment);
        string.as_str()
    };

    let mut filters = HashMap::new();
    filters.insert("label", vec![searchString]);

    let containers = &docker
        .list_containers(Some(ListContainersOptions {
            all: true,
            filters,
            ..Default::default()
        }))
        .await
        .unwrap();

    let mut x: Vec<Container> = Vec::new();

    for c in containers {
        x.push(Container {
            id: c.id.clone(),
            names: c.names.clone(),
            image: c.image.clone(),
            status: c.status.clone(),
            labels: c.labels.clone(),
            state: c.state.clone(),
        })
    }

    Some(ContainerQuery { containers: x })
}

#[command]
pub async fn restart_container(containerId: String) -> Option<String> {
    let docker = Docker::connect_with_local_defaults().expect("Failed to connect to docker");

    println!("Restarting container {}...", containerId.as_str());
    &docker
        .restart_container(containerId.as_str(), None)
        .await
        .unwrap();

    println!("Restarted container {}...", containerId.as_str());
    Some("ok".to_string())
}

#[command]
pub async fn docker_version_cmd(event: String) -> Option<String> {
    let stdout = docker_version();
    let info = AdvertiseOk { ok: stdout };
    Some(serde_json::to_string(&info).unwrap())
}

#[command]
pub async fn directory_init_cmd(event: String) -> Option<String> {
    let x: InitializeRequest = serde_json::from_str(event.as_str()).unwrap();
    let stdout = directory_init(x);
    Some(serde_json::to_string(&stdout).unwrap())
}

#[command]
pub async fn directory_up_cmd(event: String) -> Option<String> {
    let x: UpRequest = serde_json::from_str(event.as_str()).unwrap();
    let stdout = directory_up(x);
    Some(serde_json::to_string(&stdout).unwrap())
}

#[command]
pub async fn directory_stop_cmd(event: String) -> Option<String> {
    let x: StopRequest = serde_json::from_str(event.as_str()).unwrap();
    let stdout = directory_stop(x);
    Some(serde_json::to_string(&stdout).unwrap())
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ListNetworkInterfaceRequest {
    v4: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Interface {
    broadcast: Option<String>,
    name: String,
    host: String,
    bind: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ListNetworkInterfaceAnswer {
    ok: Vec<Interface>,
    error: Option<String>,
}

#[command]
pub async fn list_network_interfaces(v4: bool) -> Result<Vec<Interface>, String> {
    let network_interfaces = NetworkInterface::show().unwrap();

    let mut interfaces = Vec::new();

    for itf in network_interfaces.iter() {
        println!("{:?}", itf);

        let v4 = match itf.addr {
            Some(addr) => match addr {
                Addr::V4(t) => Some(t),
                _ => None,
            },
            None => None,
        };

        match v4 {
            Some(v4) => {
                let bind = itf.addr.unwrap().ip().to_string();
                let host = lookup_addr(&itf.addr.unwrap().ip()).unwrap();
                println!("Host: {:?}", host);

                let broadcast = match v4.broadcast {
                    Some(b) => Some(b.to_string()),
                    None => None,
                };

                interfaces.push(Interface {
                    broadcast: broadcast.clone(),
                    name: itf.name.clone(),
                    host: host.clone(),
                    bind: itf.addr.unwrap().ip().to_string(),
                });

                if (host != bind) {
                    interfaces.push(Interface {
                        broadcast: broadcast.clone(),
                        name: itf.name.clone() + "-ip",
                        host: itf.addr.unwrap().ip().to_string(),
                        bind: itf.addr.unwrap().ip().to_string(),
                    });
                }
            }
            None => {}
        }
    }

    Ok(interfaces)
}

pub fn hello_world(event: String) -> String {
    let output = if cfg!(target_os = "windows") {
        Command::new("cmd")
            .args(["/C", format!("echo {}", event.to_string()).as_str()])
            .output()
            .expect("failed to execute process")
    } else {
        Command::new("sh")
            .arg("-c")
            .arg(format!("echo {}", event.to_string()).as_str())
            .output()
            .expect("failed to execute process")
    };
    let stdout = String::from_utf8(output.stdout).unwrap();
    return stdout;
}

pub fn docker_version() -> String {
    let output = Command::new("docker")
        .arg("version")
        .output()
        .expect("failed to execute s");

    let stdout = String::from_utf8(output.stdout).unwrap();
    return stdout;
}

pub fn directory_init(x: InitializeRequest) -> InitializeAnswer {
    println!("Did that here Here");

    let dir = match canonicalize(x.dirpath.clone()) {
        Ok(dir) => dir,
        Err(e) => {
            println!("Error: {:?}", e);
            return InitializeAnswer {
                ok: None,
                error: Some(format!("Error canocializing this: {:?}", e)),
            };
        }
    };

    let config_path = dir.join("setup.yaml");

    let z = std::fs::write(config_path, x.yaml);
    match z {
        Ok(_) => {}
        Err(e) => {
            println!("Error: {:?}", e);
            return InitializeAnswer {
                ok: None,
                error: Some(format!("Error Writing File: {:?}", e)),
            };
        }
    };

    let dir_str: String = format!(
        "{}:/app/init",
        match dir.clone().into_os_string().into_string() {
            Ok(dir_str) => dir_str,
            Err(e) => {
                println!("Error: {:?}", e);
                return InitializeAnswer {
                    ok: None,
                    error: Some(format!("Error converting to to ostring: {:?}", e)),
                };
            }
        }
    );

    println!("Mounting on {}", dir_str);
    let output = if cfg!(target_os = "windows") {
        Command::new("docker")
            .current_dir(dir)
            .args([
                "run",
                "--rm",
                "-v",
                format!("{}:/app/init", x.dirpath.as_str()).as_str(),
                "jhnnsrs/guss:prod",
            ])
            .output()
    } else {
        Command::new("docker")
            .current_dir(dir)
            .args(["run", "--rm", "-v", dir_str.as_str(), "jhnnsrs/guss:prod"])
            .output()
    };

    let output = match output {
        Ok(output) => output,
        Err(e) => {
            println!("Error: {:?}", e);
            return InitializeAnswer {
                ok: None,
                error: Some(format!("Error running docker: {:?}", e)),
            };
        }
    };

    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    println!("Finished with the following {}", stdout);
    println!("Finished with the following {}", stderr);
    return InitializeAnswer {
        ok: Some(stdout),
        error: Some(format!("Error running builder {:?}", stderr)),
    };
}

pub fn directory_up(x: UpRequest) -> UpAnswer {
    println!("Did that here Here");

    let dir = canonicalize(x.dirpath.clone()).unwrap();
    let output = if cfg!(target_os = "windows") {
        Command::new("docker-compose")
            .current_dir(dir)
            .args(["up", "-d"])
            .output()
            .expect("failed to execute process")
    } else {
        Command::new("docker-compose")
            .current_dir(dir)
            .args(["up", "-d"])
            .output()
            .expect("failed to execute process")
    };

    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    println!("Finished with the following {}", stdout);
    println!("Finished with the following Error {}", stderr);
    return UpAnswer {
        ok: Some(stdout),
        error: Some(stderr),
    };
}

pub fn directory_stop(x: StopRequest) -> StopAnswer {
    println!("Did that here Here");

    let dir = canonicalize(x.dirpath.clone()).unwrap();
    let dir_str: String = format!("{}:/init", dir.to_str().unwrap().to_string());
    println!("Mounting on {}", dir_str);
    let output = if cfg!(target_os = "windows") {
        Command::new("docker-compose")
            .current_dir(dir)
            .args(["stop"])
            .output()
            .expect("failed to execute process")
    } else {
        Command::new("docker-compose")
            .current_dir(dir)
            .args(["stop"])
            .output()
            .expect("failed to execute process")
    };

    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    println!("Finished with the following {}", stdout);
    println!("Finished with the following {}", stderr);
    return StopAnswer {
        ok: Some(stdout),
        error: Some(stderr),
    };
}
