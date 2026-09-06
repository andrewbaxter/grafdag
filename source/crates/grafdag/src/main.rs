//! Local server for the grafdag editor. Serves the embedded web app, hands it
//! the document and writes the document back when the app saves it.
use {
    aargvark::{
        vark,
        Aargvark,
    },
    axum::{
        body::Body,
        extract::State,
        http::{
            header,
            StatusCode,
            Uri,
        },
        response::{
            IntoResponse,
            Response,
        },
        routing::get,
        Router,
    },
    grafdag_core::Document,
    rust_embed::RustEmbed,
    std::{
        net::SocketAddr,
        path::PathBuf,
        sync::Arc,
    },
    tokio::sync::Mutex,
};

/// First port to try; subsequent ports are tried if it's in use.
const DEFAULT_PORT: u16 = 5870;
const PORT_ATTEMPTS: u16 = 100;

#[derive(RustEmbed)]
#[folder = "$GRAFDAG_STATIC_DIR/"]
struct Static;

/// Open a DAG document in the browser for viewing and editing. The file is
/// created on first save if it doesn't exist.
#[derive(Aargvark)]
struct Args {
    /// Path to the JSON document.
    file: PathBuf,
    /// Port to listen on; if it's taken the next ports are tried in turn (default: 5870).
    port: Option<u16>,
    /// Don't open the browser automatically.
    no_open: Option<()>,
}

struct App {
    path: PathBuf,
    doc: Mutex<Document>,
}

async fn get_doc(State(app): State<Arc<App>>) -> Response {
    let doc = app.doc.lock().await;
    let body = serde_json::to_string(&*doc).unwrap();
    return ([(header::CONTENT_TYPE, "application/json")], body).into_response();
}

async fn post_doc(State(app): State<Arc<App>>, body: String) -> Response {
    let doc = match serde_json::from_str::<Document>(&body) {
        Ok(d) => d,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("Invalid document: {}", e)).into_response();
        },
    };
    let mut current = app.doc.lock().await;
    let pretty = serde_json::to_string_pretty(&doc).unwrap();
    let tmp = app.path.with_extension("json.tmp");
    if let Err(e) = tokio::fs::write(&tmp, pretty).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to write {}: {}", tmp.display(), e)).into_response();
    }
    if let Err(e) = tokio::fs::rename(&tmp, &app.path).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to replace {}: {}", app.path.display(), e)).into_response();
    }
    *current = doc;
    eprintln!("Saved {}", app.path.display());
    return StatusCode::NO_CONTENT.into_response();
}

/// The index page, with the current document inlined so the app renders
/// without a round trip.
async fn index(State(app): State<Arc<App>>) -> Response {
    let Some(file) = Static::get("index.html") else {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    };
    let html = String::from_utf8_lossy(&file.data);
    let doc = app.doc.lock().await;
    // Escape so the JSON can't terminate the script element
    let json = serde_json::to_string(&*doc).unwrap().replace("</", "<\\/");
    let html = html.replace("<!--GRAFDAG_DOC-->", &json);
    return Response::builder()
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(html))
        .unwrap();
}

async fn static_file(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let Some(file) = Static::get(path) else {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    };
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    return Response::builder()
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(file.data.into_owned()))
        .unwrap();
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
    let router = Router::new()
        .route("/", get(index))
        .route("/index.html", get(index))
        .route("/api/doc", get(get_doc).post(post_doc))
        .fallback(static_file)
        .with_state(app);
    let base_port = args.port.unwrap_or(DEFAULT_PORT);
    let mut listener = None;
    for port in base_port .. base_port.saturating_add(PORT_ATTEMPTS) {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        match tokio::net::TcpListener::bind(addr).await {
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
    axum::serve(listener, router).await.unwrap();
}
