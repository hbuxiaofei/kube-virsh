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
    arch: String,
    node_name: String,
    ns: String,
    pod_name: String,
    pod_status: String,
    vm_status: String,
    start_time: String,
}

pub async fn pod_list(prefix_ns: &str, prefix_pod: &str, with_virsh: bool) -> anyhow::Result<()> {
    let mut pod_map = HashMap::new();

    let client = Client::try_default().await?;

    type PodSimple = Object<PodSpec, PodStatus>;

    // Here we simply steal the type info from k8s_openapi, but we could create this from scratch.
    let ar = ApiResource::erase::<k8s_openapi::api::core::v1::Pod>(&());
    let pod_simple: Api<PodSimple> = Api::all_with(client.clone(), &ar);

    for p in pod_simple.list(&Default::default()).await? {
        // println!(">>>Pod: {:?}", p);

        // log::info!("Found pod {} running: {:?}", p.name(), p.spec.containers);
        let ns = p.metadata.namespace.unwrap();

        if ns.starts_with(prefix_ns) {
            let pod_name = p.metadata.name.unwrap();
            if !pod_name.starts_with(prefix_pod) {
                continue;
            }

            let node_selector = p.spec.node_selector.unwrap();
            let pod_arch = node_selector
                .get("kubernetes.io/arch")
                .unwrap_or(&"None".to_string())
                .replace('"', "");

            let node_name = p.spec.node_name.unwrap_or("None".to_string());

            // let pod_status = p.status.unwrap().phase.unwrap();
            let container_statuses = p
                .status
                .as_ref()
                .unwrap()
                .container_statuses
                .as_ref()
                .unwrap();
            let state = container_statuses.get(0).unwrap().state.as_ref().unwrap();
            let mut pod_status = String::from("None");
            if state.running.is_some() {
                pod_status = String::from("Running");
            } else if state.terminated.is_some() {
                pod_status = state
                    .terminated
                    .as_ref()
                    .unwrap()
                    .reason
                    .as_ref()
                    .unwrap()
                    .to_string();
            } else if state.waiting.is_some() {
                // pod_status = String::from("Waiting");
                pod_status = state
                    .waiting
                    .as_ref()
                    .unwrap()
                    .reason
                    .as_ref()
                    .unwrap()
                    .to_string();
            }

            let start_time = p.status.unwrap().start_time.unwrap();

            let pod_info = PodInfo {
                arch: pod_arch,
                node_name: node_name,
                ns: ns.to_string(),
                pod_name: pod_name.to_string(),
                pod_status: pod_status,
                vm_status: String::from(""),
                start_time: format!("{:?}", start_time.0),
            };
            pod_map.insert(pod_name.to_string(), pod_info);
        }
    }

    // for (_, v) in &mut pod_map {
    //     if v.pod_status == "Running" {
    //         let vm_status = get_vm_status(client.clone(), &v.ns, &v.pod_name).await;
    //         v.vm_status = vm_status.unwrap();
    //     }
    // }

    if with_virsh {
        let mut fs = Vec::new();
        for (_, v) in &mut pod_map {
            if v.pod_status == "Running" {
                let f = async {
                    let vm_status = get_vm_status(client.clone(), &v.ns, &v.pod_name).await;
                    v.vm_status = vm_status.unwrap_or("None".to_string());
                };
                fs.push(f); // futures::executor::block_on(f);
            }
        }
        futures::future::join_all(fs).await;
    }

    for (_, v) in &pod_map {
        let mut status = format!("{}", v.pod_status);
        if with_virsh {
            status = format!("{}/{}", v.pod_status, v.vm_status);
            println!(
                "{:<8}{:<25}{:<25}{:<25}{:<20}{:<25}",
                v.arch, v.node_name, v.ns, v.pod_name, status, v.start_time
            );
        } else {
            println!(
                "{:<25}{:<25}{:<25}{:<20}{:<25}",
                v.node_name, v.ns, v.pod_name, status, v.start_time
            );
        }
    }
    Ok(())
}

async fn get_vm_status(client: Client, ns: &str, pod_name: &str) -> Result<String, kube::Error> {
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

    let attached = pods
        .exec(
            &pod_name,
            vec!["bash", "-c", "virsh --quiet list --all"],
            &AttachParams::default().stderr(false),
        )
        .await?;
    let output = get_output(attached).await;
    if output.len() > 0 {
        let pos: Vec<&str> = output.split(" ").collect();
        return Ok(pos[pos.len() - 1].replace('\n', ""));
    }
    Ok(String::from(""))
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
