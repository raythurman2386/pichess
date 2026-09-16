mod app;

use pichess::cli::{parse_cli, usage, CliAction};
use pichess::icons::AppAssets;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match parse_cli(&args) {
        Ok(CliAction::Version) => {
            println!("pichess {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Ok(CliAction::Help) => {
            println!("{}", usage());
            return;
        }
        Ok(CliAction::Perft(depth)) => {
            let mut position = pichess::Position::new();
            let start = std::time::Instant::now();
            let nodes = position.perft(depth);
            let elapsed = start.elapsed();
            let nps = if elapsed.as_secs_f64() > 0.0 {
                (nodes as f64 / elapsed.as_secs_f64()) as u64
            } else {
                0
            };
            println!("perft({depth}) = {nodes} in {:.2?} ({nps} nps)", elapsed);
            return;
        }
        Ok(CliAction::Run) => {}
        Err(err) => {
            eprintln!(
                "pichess: unrecognized argument: {}\n{}",
                err.argument,
                usage()
            );
            std::process::exit(2);
        }
    }

    let app = gpui_kit::application().with_assets(AppAssets);
    app.run(|cx| {
        gpui_kit::init(cx);
        app::init(cx);
        cx.activate(true);

        cx.spawn(async move |cx| {
            app::open_window(cx).expect("failed to open window");
        })
        .detach();
    });
}
