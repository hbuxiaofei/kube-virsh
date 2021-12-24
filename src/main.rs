use env_logger::Builder;
use log;
use std::io::Write;

mod exec;
mod list;

#[tokio::main]
async fn main() {
    std::env::set_var("RUST_LOG", "info,kube=info");

    // env_logger::init();
    let mut builder = Builder::from_default_env();
    builder
        .format(|buf, record| writeln!(buf, "{}: {}", record.level(), record.args()))
        .filter(None, log::LevelFilter::Info)
        .init();

    let matches = clap::App::new("Kube virsh command")
        .version("0.1.0")
        .author("Anonymous <anonymous@anonymous.net>")
        .arg(
            clap::Arg::with_name("pod")
                .short("p")
                .long("pod")
                .takes_value(true)
                .help("The pod type. Value is ecs or agent, Default is ecs."),
        )
        .subcommand(
            clap::SubCommand::with_name("list")
                .about("The list command to run.")
                .version("0.1.0")
                .author("Anonymous <anonymous@anonymous.net>"),
        )
        .subcommand(
            clap::SubCommand::with_name("getxml")
                .about("The getxml command to run.")
                .version("0.1.0")
                .author("Anonymous <anonymous@anonymous.net>")
                .arg(
                    clap::Arg::with_name("name")
                        .index(1)
                        .takes_value(true)
                        .help("The name of pod."),
                ),
        )
        .subcommand(
            clap::SubCommand::with_name("logs")
                .about("The logs command to run.")
                .version("0.1.0")
                .author("Anonymous <anonymous@anonymous.net>")
                .arg(
                    clap::Arg::with_name("name")
                        .index(1)
                        .takes_value(true)
                        .help("The name of pod."),
                ),
        )
        .get_matches();

    let pod_type = matches.value_of("pod").unwrap_or("ecs");

    if pod_type == "ecs" {
        if let Some(_) = matches.subcommand_matches("list") {
            if let Err(e) = list::pod_list("tenant-", "ecs-", true).await {
                log::error!("{}", e);
            }
        } else if let Some(matches) = matches.subcommand_matches("getxml") {
            let pod_short = matches.value_of("name").unwrap_or("None");
            if let Err(e) = exec::pod_exec(pod_short, "getxml").await {
                log::error!("{}", e);
            }
        } else if let Some(matches) = matches.subcommand_matches("logs") {
            let pod_short = matches.value_of("name").unwrap_or("None");
            if let Err(e) = exec::pod_exec(pod_short, "logs").await {
                log::error!("{}", e);
            }
        }
    } else if pod_type == "agent" {
        if let Some(_) = matches.subcommand_matches("list") {
            if let Err(e) = list::pod_list("product-ecs", "ecs-node-agent-", false).await {
                log::error!("{}", e);
            }
        }
    } else {
        log::error!("Unknown pod type {}", pod_type);
    }
}
