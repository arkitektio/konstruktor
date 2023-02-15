use bollard::image::ListImagesOptions;
use bollard::{Docker, API_DEFAULT_VERSION};
use dns_lookup::lookup_addr;
use network_interface::NetworkInterface;
use network_interface::NetworkInterfaceConfig;
use serde::{Deserialize, Serialize};
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
pub async fn advertise_endpoint(event: String) -> Option<String> {
    let x: Endpoint = serde_json::from_str(event.as_str()).unwrap();

    let socket: UdpSocket = UdpSocket::bind("0.0.0.0:0").unwrap();
    socket.set_read_timeout(Some(Duration::new(5, 0))).unwrap();
    socket.set_broadcast(true).unwrap();
    println!("Connected on port {}", x.docker_addr);
    println!("Broadcast: {:?}", socket.broadcast());
    println!("Timeout: {:?}", socket.read_timeout());

    let call: Vec<u8> = "packer.get_buf()?".as_bytes().to_vec();
    println!("Sending call, {} bytes", call.len());
    socket.send_to(&call, "255.255.255.255:45678").unwrap();

    let info = AdvertiseOk {
        ok: "ok".to_string(),
    };

    Some(serde_json::to_string(&info).unwrap())
}

#[command]
pub async fn nana_test(event: String) -> Option<String> {
    let docker = Docker::connect_with_local_defaults().expect("Failed to connect to docker");

    let version = docker.version().await.unwrap();
    let images = &docker
        .list_images(Some(ListImagesOptions::<String> {
            all: true,
            ..Default::default()
        }))
        .await
        .unwrap();

    let mut x = String::new();

    for image in images {
        x = image.id.clone();
    }

    Some(x)
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
    name: String,
    host: String,
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
            Some(addr) => addr.ip().is_ipv4(),
            None => false,
        };

        if v4 {
            let host = lookup_addr(&itf.addr.unwrap().ip()).unwrap();
            println!("Host: {:?}", host);

            interfaces.push(Interface {
                name: itf.name.clone(),
                host: host,
            });

            interfaces.push(Interface {
                name: itf.name.clone() + "-ip",
                host: itf.addr.unwrap().ip().to_string(),
            });
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
