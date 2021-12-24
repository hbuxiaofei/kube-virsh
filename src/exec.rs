use futures::{StreamExt, TryStreamExt};
use log;
use std::collections::HashMap;

use kube::{
    api::{Api, AttachParams, AttachedProcess, ListParams, Object, WatchEvent},
    discovery::ApiResource,
    Client,
};

use k8s_openapi::api::core::v1::{Pod, PodSpec, PodStatus};

struct PodInfo {
    ns: String,
    pod_name: String,
}

pub async fn pod_exec(pod_short: &str, command: &str) -> anyhow::Result<()> {
    let mut pod_map = HashMap::new();

    let client = Client::try_default().await?;

    type PodSimple = Object<PodSpec, PodStatus>;

    // Here we simply steal the type info from k8s_openapi, but we could create this from scratch.
    let ar = ApiResource::erase::<k8s_openapi::api::core::v1::Pod>(&());
    let pod_simple: Api<PodSimple> = Api::all_with(client.clone(), &ar);

    for p in pod_simple.list(&Default::default()).await? {
        let ns = p.metadata.namespace.unwrap();

        if ns.starts_with("tenant-") {
            let pod_name = p.metadata.name.unwrap();
            if !pod_name.contains(pod_short) {
                continue;
            }

            let pod_info = PodInfo {
                pod_name: pod_name.to_string(),
                ns: ns.to_string(),
            };

            pod_map.insert(pod_name, pod_info);
        }
    }

    for (_, v) in &pod_map {
        if let Ok(v) = get_exec_output(client.clone(), &v.ns, &v.pod_name, command).await {
            println!("{}", v);
        }
    }
    Ok(())
}

fn convert_command(cmd: &str, pod_name: &str) -> Option<String> {
    let pos: Vec<&str> = pod_name.split("-").collect();
    if pos.len() < 2 {
        return None;
    }

    if cmd == "getxml" {
        return Some(format!("virsh dumpxml {}-{} --inactive", pos[0], pos[1]));
    } else if cmd == "logs" {
        return Some(format!("cat /var/log/libvirt/libvirtd.log"));
    }

    return None;
}

async fn get_exec_output(
    client: Client,
    ns: &str,
    pod_name: &str,
    command: &str,
) -> Result<String, kube::Error> {
    let filter = format!("metadata.name={}", &pod_name);
    let lp = ListParams::default().fields(&filter).timeout(3);

    let pods: Api<Pod> = Api::namespaced(client.clone(), &ns);
    let mut stream = pods.watch(&lp, "0").await?.boxed();

    while let Some(status) = stream.try_next().await? {
        match status {
            WatchEvent::Added(_) => {
                // log::info!("Added {}", o.name());
                break;
            }
            _ => {
                log::info!("{:?}", status);
                break;
            }
        }
    }

    if let Some(cmd) = convert_command(command, pod_name) {
        let attached = pods
            .exec(
                &pod_name,
                vec!["bash", "-c", cmd.as_str()],
                &AttachParams::default().stderr(false),
            )
            .await?;
        let output = get_output(attached).await;
        return Ok(output);
    }

    Ok("".to_string())
}

async fn get_output(mut attached: AttachedProcess) -> String {
    let stdout = tokio_util::io::ReaderStream::new(attached.stdout().unwrap());
    let out = stdout
        .filter_map(|r| async { r.ok().and_then(|v| String::from_utf8(v.to_vec()).ok()) })
        .collect::<Vec<_>>()
        .await
        .join("");
    attached.await;
    out
}
