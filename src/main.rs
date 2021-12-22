use k8s_openapi::api::core::v1::{Pod, PodSpec, PodStatus};

use futures::{StreamExt, TryStreamExt};

use kube::{
    api::{Api, AttachParams, AttachedProcess, ListParams, Object, WatchEvent},
    Client,
};

use kube::discovery::ApiResource;

use log::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    std::env::set_var("RUST_LOG", "info,kube=info");

    env_logger::init();

    let client = Client::try_default().await?;

    type PodSimple = Object<PodSpec, PodStatus>;

    // Here we simply steal the type info from k8s_openapi, but we could create this from scratch.
    let ar = ApiResource::erase::<k8s_openapi::api::core::v1::Pod>(&());
    let pod_simple: Api<PodSimple> = Api::all_with(client.clone(), &ar);

    for p in pod_simple.list(&Default::default()).await? {
        // info!("Found pod {} running: {:?}", p.name(), p.spec.containers);
        let ns = p.metadata.namespace.as_ref().unwrap();

        if ns.starts_with("tenant-") {
            let node_selector = p.spec.node_selector.as_ref().unwrap();
            let pod_arch = node_selector
                .get("kubernetes.io/arch")
                .unwrap()
                .replace('"', "");

            let node_name = p.spec.node_name.as_ref().unwrap();

            let pod_name = p.metadata.name.as_ref().unwrap();

            let pod_status = p.status.unwrap().phase.unwrap();

            let mut status = format!("{}/-", pod_status);
            if pod_status == "Running" {
                let vm_status = get_vm_status(client.clone(), &ns, &pod_name).await?;
                if vm_status.len() > 0 {
                    status = format!("{}/{}", pod_status, vm_status);
                }
            }

            println!(
                "{:<8}{:<32}{:<32}{:<32}{:<10}",
                pod_arch, node_name, ns, &pod_name, status
            );
            // println!(">>> {:?}", p);
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
                // info!("Added {}", o.name());
                break;
            }
            _ => {
                info!("{:?}", status);
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
