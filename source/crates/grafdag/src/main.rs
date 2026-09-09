mod serve;

use {
    aargvark::{
        Aargvark,
        vark,
    },
    grafdag_core::Document,
    std::path::PathBuf,
};

#[derive(Aargvark)]
struct Args {
    /// Path to the JSON document.
    file: PathBuf,
    /// Open the document in the system browser instead of a desktop window.
    browser: Option<()>,
    /// Just serve; don't open a window or a browser.
    no_open: Option<()>,
    /// Port to listen on; if it's taken the next ports are tried in turn (default:
    /// 5870).
    port: Option<u16>,
}

fn main() {
    let args = vark::<Args>();
    let doc = match std::fs::read(&args.file) {
        Ok(bytes) => match serde_json::from_slice::<Document>(&bytes) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("Failed to parse {}: {}", args.file.display(), e);
                std::process::exit(1);
            },
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("{} doesn't exist, starting with an empty document", args.file.display());
            Document::default()
        },
        Err(e) => {
            eprintln!("Failed to read {}: {}", args.file.display(), e);
            std::process::exit(1);
        },
    };
    let title = format!("{} - grafdag", match args.file.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => args.file.display().to_string(),
    });

    // Tauri owns the main thread, so build the runtime by hand and keep it alive for
    // the duration of the process.
    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("Failed to start the async runtime: {}", e);
            std::process::exit(1);
        },
    };
    let url = match rt.block_on(serve::start(doc, args.file, args.port.unwrap_or(5870))) {
        Ok(url) => url,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        },
    };
    eprintln!("Serving at {}", url);
    if args.no_open.is_some() {
        rt.block_on(std::future::pending::<()>());
        return;
    }
    if args.browser.is_some() {
        if let Err(e) = open::that_detached(&url) {
            eprintln!("Failed to open browser: {}", e);
            std::process::exit(1);
        }
        rt.block_on(std::future::pending::<()>());
        return;
    }
    let _guard = rt.enter();
    let url = match url.parse() {
        Ok(url) => url,
        Err(e) => {
            eprintln!("Bad server url {}: {}", url, e);
            std::process::exit(1);
        },
    };
    let res =
        tauri::Builder::default()
            .setup(move |app| {
                tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::External(url))
                    .title(title)
                    .inner_size(1200., 800.)
                    .build()?;
                return Ok(());
            })
            .run(tauri::generate_context!());
    if let Err(e) = res {
        eprintln!("Failed to open the window: {}", e);
        eprintln!("(pass --browser to use the system browser instead)");
        std::process::exit(1);
    }
}
