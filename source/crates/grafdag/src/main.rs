use {
    aargvark::{
        Aargvark,
        vark,
    },
    grafdag_core::Document,
    http::{
        StatusCode,
        header::{
            CACHE_CONTROL,
            CONTENT_TYPE,
        },
    },
    http_body_util::BodyExt,
    htwrap::htserve::responses::{
        Body,
        body_empty,
        body_full,
        response_200_json,
        response_400,
        response_404,
    },
    hyper::{
        Method,
        Request,
        Response,
        body::Incoming,
        server::conn::http1,
        service::service_fn,
    },
    hyper_util::rt::TokioIo,
    rust_embed::RustEmbed,
    std::{
        net::SocketAddr,
        path::PathBuf,
        sync::Arc,
    },
    tokio::{
        net::TcpListener,
        sync::Mutex,
    },
};

struct App {
    doc: Mutex<Document>,
    path: PathBuf,
}

#[derive(Aargvark)]
struct Args {
    /// Path to the JSON document.
    file: PathBuf,
    /// Don't open the browser automatically.
    no_open: Option<()>,
    /// Port to listen on; if it's taken the next ports are tried in turn (default:
    /// 5870).
    port: Option<u16>,
}

#[tokio::main]
async fn main() {
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
    let app = Arc::new(App {
        path: args.file,
        doc: Mutex::new(doc),
    });
    let base_port = args.port.unwrap_or(5870);
    const PORT_ATTEMPTS: u16 = 100;
    let mut listener = None;
    for port in base_port .. base_port.saturating_add(PORT_ATTEMPTS) {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        match TcpListener::bind(addr).await {
            Ok(l) => {
                listener = Some(l);
                break;
            },
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                continue;
            },
            Err(e) => {
                eprintln!("Failed to listen on {}: {}", addr, e);
                std::process::exit(1);
            },
        }
    }
    let Some(listener) = listener else {
        eprintln!("No free port in {}..{}", base_port, base_port.saturating_add(PORT_ATTEMPTS));
        std::process::exit(1);
    };
    let url = format!("http://{}/", listener.local_addr().unwrap());
    eprintln!("Serving at {}", url);
    if args.no_open.is_none() {
        if let Err(e) = open::that_detached(&url) {
            eprintln!("Failed to open browser: {}", e);
        }
    }
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(x) => x,
            Err(e) => {
                eprintln!("Failed to accept connection: {}", e);
                continue;
            },
        };
        let io = TokioIo::new(stream);
        let app = app.clone();
        tokio::task::spawn(async move {
            let service = service_fn(move |req: Request<Incoming>| {
                let app = app.clone();
                async move {
                    let path = req.uri().path().to_string();
                    let response = match (req.method(), path.as_str()) {
                        (&Method::GET, "/") | (&Method::GET, "/index.html") => {
                            match Static::get("index.html") {
                                Some(file) => {
                                    let html = String::from_utf8_lossy(&file.data);
                                    let doc = app.doc.lock().await;
                                    let json = serde_json::to_string(&*doc).unwrap().replace("</", "<\\/");
                                    let html = html.replace("<!--GRAFDAG_DOC-->", &json);
                                    Response::builder()
                                        .header(CONTENT_TYPE, "text/html; charset=utf-8")
                                        .header(CACHE_CONTROL, "no-cache")
                                        .body(body_full(html.into_bytes()))
                                        .unwrap()
                                },
                                None => response_404(),
                            }
                        },
                        (&Method::GET, "/api/doc") => {
                            let doc = app.doc.lock().await;
                            response_200_json(&*doc)
                        },
                        (&Method::POST, "/api/doc") => 'save: {
                            let bytes = match req.into_body().collect().await {
                                Ok(collected) => collected.to_bytes(),
                                Err(e) => break 'save response_400(format!("Error reading body: {}", e)),
                            };
                            let doc = match serde_json::from_slice::<Document>(&bytes) {
                                Ok(d) => d,
                                Err(e) => break 'save response_400(format!("Invalid document: {}", e)),
                            };
                            let mut current = app.doc.lock().await;
                            let pretty = serde_json::to_string_pretty(&doc).unwrap();
                            let tmp = app.path.with_extension("json.tmp");
                            if let Err(e) = tokio::fs::write(&tmp, pretty).await {
                                break 'save response_500(format!("Failed to write {}: {}", tmp.display(), e));
                            }
                            if let Err(e) = tokio::fs::rename(&tmp, &app.path).await {
                                break 'save response_500(format!("Failed to replace {}: {}", app.path.display(), e));
                            }
                            *current = doc;
                            eprintln!("Saved {}", app.path.display());
                            Response::builder().status(StatusCode::NO_CONTENT).body(body_empty()).unwrap()
                        },
                        _ => {
                            let path = path.trim_start_matches('/');
                            match Static::get(path) {
                                Some(file) => {
                                    let mime = mime_guess::from_path(path).first_or_octet_stream();
                                    Response::builder()
                                        .header(CONTENT_TYPE, mime.as_ref())
                                        .header(CACHE_CONTROL, "no-cache")
                                        .body(body_full(file.data.into_owned()))
                                        .unwrap()
                                },
                                None => response_404(),
                            }
                        },
                    };
                    Ok::<_, std::io::Error>(response)
                }
            });
            if let Err(e) = http1::Builder::new().serve_connection(io, service).with_upgrades().await {
                eprintln!("Error serving connection: {}", e);
            }
        });
    }
}

fn response_500(message: impl ToString) -> Response<Body> {
    return Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(body_full(message.to_string().into_bytes()))
        .unwrap();
}
#[derive(RustEmbed)]
#[folder = "$GRAFDAG_STATIC_DIR/"]
struct Static;
