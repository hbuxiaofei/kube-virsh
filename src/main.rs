use k8s_openapi::api::core::v1::{Pod, PodSpec};

use futures::{StreamExt, TryStreamExt};

use kube::{
    api::{
        Api, AttachParams, AttachedProcess, ListParams, NotUsed, Object, ResourceExt, WatchEvent,
    },
    Client,
};

use kube::discovery::ApiResource;

use log::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    std::env::set_var("RUST_LOG", "info,kube=info");

    env_logger::init();

    let client = Client::try_default().await?;

    type PodSimple = Object<PodSpec, NotUsed>;

    // Here we simply steal the type info from k8s_openapi, but we could create this from scratch.
    let ar = ApiResource::erase::<k8s_openapi::api::core::v1::Pod>(&());

    let pods: Api<PodSimple> = Api::all_with(client, &ar);
    for p in pods.list(&Default::default()).await? {
        // info!("Found pod {} running: {:?}", p.name(), p.spec.containers);
        let ns = p.metadata.namespace.as_ref().unwrap();

        if ns.starts_with("tenant-") {
            let pod_name = p.metadata.name.as_ref().unwrap();
            let node_name = p.spec.node_name.as_ref().unwrap();
            let node_selector = p.spec.node_selector.as_ref().unwrap();

            // println!("{:<32}{:<32}{:<32}", node_name, ns, &pod_name);

            let client2 = Client::try_default().await?;
            let pod2s: Api<Pod> = Api::namespaced(client2.clone(), &ns);

            let filter = format!("metadata.name={}", &pod_name);
            let lp = ListParams::default().fields(&filter).timeout(3);

            let mut stream = pod2s.watch(&lp, "0").await?.boxed();

            while let Some(status) = stream.try_next().await? {
                match status {
                    WatchEvent::Added(_) => {
                        // info!("Added {}", o.name());
                        break;
                    }
                    WatchEvent::Modified(o) => {
                        let s = o.status.as_ref().expect("status exists on pod");
                        if s.phase.clone().unwrap_or_default() == "Running" {
                            info!("Ready to attach to {}", o.name());
                            break;
                        }
                    }
                    _ => {}
                }
            }

            let attached = pod2s
                .exec(
                    &pod_name,
                    vec!["bash", "-c", "virsh --quiet list --all"],
                    &AttachParams::default().stderr(false),
                )
                .await?;
            let output = get_output(attached).await;

            let pod_arch = node_selector
                .get("kubernetes.io/arch")
                .unwrap()
                .replace('\"', "");
            if output.len() > 0 {
                let pos: Vec<&str> = output.split(" ").collect();

                println!(
                    "{:<8}{:<32}{:<32}{:<32}{:<10}",
                    pod_arch,
                    node_name,
                    ns,
                    &pod_name,
                    pos[pos.len() - 1].replace('\n', "")
                );
            } else {
                println!(
                    "{:<8}{:<32}{:<32}{:<32}-",
                    pod_arch, node_name, ns, &pod_name
                );
            }
        }
    }

    Ok(())
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
