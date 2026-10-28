//! Modelo servido pelo `llama-server` do llama.cpp (ADR-022): o gateway é o nó principal, carrega o
//! GGUF e divide as camadas com os workers pelo RPC do llama.cpp. Cada worker é alcançado por um túnel
//! mTLS local (`ai-cluster-tunnel`): o `llama-server` só enxerga portas em 127.0.0.1.
//!
//! Ligado por `AI_CLUSTER_LLAMA_MODEL` (caminho do .gguf). Os endpoints dos workers vêm de
//! `AI_CLUSTER_LLAMA_RPC_ENDPOINTS` (lista `host:porta` dos túneis dos workers); sem eles o modelo
//! roda só na CPU do gateway.

use std::pin::Pin;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes};
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use futures_core::Stream;
use hyper_util::client::legacy::{Client, connect::HttpConnector};
use hyper_util::rt::TokioExecutor;

use ai_cluster_protocol::cluster::{DesiredModel, ModelState, ModelStatus};

use crate::request_manager::{Permit, Tenant};

type HttpClient = Client<HttpConnector, Body>;

/// Quanto esperar o modelo ficar pronto: carregar o GGUF e empurrar os tensores a um Pi leva minutos.
const LOAD_TIMEOUT: Duration = Duration::from_secs(1800);
/// Primeira porta local dos túneis, uma por worker.
const DEFAULT_TUNNEL_BASE: u16 = 18100;
const DEFAULT_PORT: u16 = 18080;
/// Prompt fixo do benchmark e quantos tokens ele gera.
const BENCH_PROMPT: &str = "Explique em poucas frases o que é um cluster de computadores.";
const BENCH_TOKENS: u32 = 64;
/// Maior contexto que o plano do controller pode pedir (1 M de tokens).
const MAX_CONTEXT: u32 = 1 << 20;

/// A configuração lida do ambiente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlamaConfig {
    pub id: String,
    pub model: String,
    pub server_bin: String,
    pub endpoints: Vec<String>,
    pub ca: String,
    pub cert: String,
    pub key: String,
    pub server_name: String,
    pub context: u32,
    pub gpu_layers: u32,
    pub port: u16,
    pub tunnel_base: u16,
    /// Gerações ao mesmo tempo no llama-server (`-np`); o contexto de cada uma é `context`.
    pub slots: u32,
}

impl LlamaConfig {
    /// Argumentos do `llama-server`: loopback apenas, uma vaga, e os túneis locais como `--rpc`.
    pub fn server_args(&self) -> Vec<String> {
        let mut args: Vec<String> = [
            "-m",
            &self.model,
            "--host",
            "127.0.0.1",
            "--port",
            &self.port.to_string(),
            "-c",
            // O `-c` do llama-server é o total, dividido entre as vagas.
            &self.context.saturating_mul(self.slots.max(1)).to_string(),
            "-np",
            &self.slots.max(1).to_string(),
            "-a",
            &self.id,
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        if !self.endpoints.is_empty() {
            args.push("-ngl".into());
            args.push(self.gpu_layers.to_string());
            args.push("--rpc".into());
            // `start` já validou a faixa; sem ela o `--rpc` ficaria vazio e o erro apareceria lá.
            args.push(self.local_endpoints().unwrap_or_default().join(","));
        }
        args
    }

    /// Os túneis locais, na ordem dos endpoints dos workers. Erro se a faixa passa de 65535 ou
    /// atravessa a porta do próprio `llama-server`.
    pub fn local_endpoints(&self) -> Result<Vec<String>, String> {
        (0..self.endpoints.len())
            .map(|i| {
                let port = u16::try_from(i)
                    .ok()
                    .and_then(|i| self.tunnel_base.checked_add(i))
                    .filter(|port| *port != self.port)
                    .ok_or_else(|| "faixa de portas dos túneis inválida".to_string())?;
                Ok(format!("127.0.0.1:{port}"))
            })
            .collect()
    }
}

/// Lê o ambiente. `None` quando o gateway não serve modelo do llama.cpp; `Err` quando pediu e a
/// configuração é insegura ou inválida.
pub fn from_env() -> Result<Option<LlamaConfig>, String> {
    let var = |name: &str| {
        std::env::var(name)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    let Some(model) = var("AI_CLUSTER_LLAMA_MODEL") else {
        return Ok(None);
    };
    let id = var("AI_CLUSTER_LLAMA_MODEL_ID").unwrap_or_else(|| {
        std::path::Path::new(&model)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "llama".into())
    });
    if id.len() > 64
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    {
        return Err(format!("AI_CLUSTER_LLAMA_MODEL_ID inválido: {id}"));
    }
    let endpoints: Vec<String> = var("AI_CLUSTER_LLAMA_RPC_ENDPOINTS")
        .map(|list| {
            list.split(',')
                .map(|e| e.trim().to_string())
                .filter(|e| !e.is_empty())
                .collect()
        })
        .unwrap_or_default();
    if endpoints.iter().any(|e| {
        e.rsplit(':')
            .next()
            .and_then(|p| p.parse::<u16>().ok())
            .is_none()
    }) {
        return Err("AI_CLUSTER_LLAMA_RPC_ENDPOINTS: use host:porta separados por vírgula".into());
    }
    let tls = (
        var("AI_CLUSTER_LLAMA_TLS_CA"),
        var("AI_CLUSTER_LLAMA_TLS_CERT"),
        var("AI_CLUSTER_LLAMA_TLS_KEY"),
    );
    let (ca, cert, key) = match tls {
        (Some(ca), Some(cert), Some(key)) => (ca, cert, key),
        _ if endpoints.is_empty() => Default::default(),
        _ => {
            return Err(
                "AI_CLUSTER_LLAMA_RPC_ENDPOINTS pede também AI_CLUSTER_LLAMA_TLS_CA, _CERT e _KEY: o RPC não vai em texto puro"
                    .into(),
            );
        }
    };
    let number = |name: &str, default: u32| -> Result<u32, String> {
        match var(name) {
            Some(text) => text.parse().map_err(|_| format!("{name} inválido: {text}")),
            None => Ok(default),
        }
    };
    let port = u16::try_from(number("AI_CLUSTER_LLAMA_PORT", u32::from(DEFAULT_PORT))?)
        .map_err(|_| "AI_CLUSTER_LLAMA_PORT fora de 0..65535".to_string())?;
    let tunnel_base = u16::try_from(number(
        "AI_CLUSTER_LLAMA_TUNNEL_BASE",
        u32::from(DEFAULT_TUNNEL_BASE),
    )?)
    .map_err(|_| "AI_CLUSTER_LLAMA_TUNNEL_BASE fora de 0..65535".to_string())?;
    if u32::from(tunnel_base) + endpoints.len() as u32 > u32::from(u16::MAX) {
        return Err("AI_CLUSTER_LLAMA_TUNNEL_BASE deixa poucas portas".into());
    }
    Ok(Some(LlamaConfig {
        id,
        model,
        server_bin: var("AI_CLUSTER_LLAMA_SERVER_BIN").unwrap_or_else(|| "llama-server".into()),
        endpoints,
        ca,
        cert,
        key,
        server_name: var("AI_CLUSTER_LLAMA_RPC_SERVER_NAME").unwrap_or_else(|| "aic-worker".into()),
        context: number("AI_CLUSTER_LLAMA_CTX", 4096)?,
        gpu_layers: number("AI_CLUSTER_LLAMA_NGL", 99)?,
        port,
        tunnel_base,
        slots: slots_from_env()?,
    }))
}

/// `AI_CLUSTER_LLAMA_SLOTS`: quantas gerações o llama-server atende ao mesmo tempo (1 a 16).
pub fn slots_from_env() -> Result<u32, String> {
    match std::env::var("AI_CLUSTER_LLAMA_SLOTS") {
        Ok(text) if !text.trim().is_empty() => text
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|n| (1..=16).contains(n))
            .ok_or_else(|| format!("AI_CLUSTER_LLAMA_SLOTS deve ir de 1 a 16, veio {text}")),
        _ => Ok(1),
    }
}

/// Em que ponto está o modelo.
#[derive(Debug, Clone, PartialEq)]
pub enum State {
    Loading,
    Ready,
    Failed(String),
}

/// O modelo servido pelo `llama-server`.
pub struct Llama {
    pub id: String,
    pub context: u32,
    port: u16,
    state: Mutex<State>,
    tokens_per_second: Mutex<f64>,
    client: HttpClient,
    /// Túneis e supervisor; abortados quando o modelo sai (o filho morre junto: `kill_on_drop`).
    tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    /// A geração do plano do controller que este processo aplica (0 = configuração do ambiente).
    pub generation: u64,
}

impl Llama {
    /// Encerra os túneis e o supervisor (e, com ele, o `llama-server`). As tarefas guardam um
    /// `Arc<Llama>`, então só este método as encerra: quem solta o modelo deve chamá-lo antes. Devolve
    /// as tarefas abortadas para quem precisar esperar o fim delas.
    pub fn shutdown(&self) -> Vec<tokio::task::JoinHandle<()>> {
        let tasks: Vec<_> = self
            .tasks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .drain(..)
            .collect();
        for task in &tasks {
            task.abort();
        }
        tasks
    }

    pub fn state(&self) -> State {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn set(&self, state: State) {
        *self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = state;
    }

    /// Tokens por segundo medidos no último benchmark; zero = ainda não medido.
    pub fn tokens_per_second(&self) -> f64 {
        *self
            .tokens_per_second
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    async fn healthy(&self) -> bool {
        let request = Request::builder()
            .uri(format!("http://127.0.0.1:{}/health", self.port))
            .body(Body::empty())
            .expect("requisição válida");
        matches!(
            tokio::time::timeout(Duration::from_secs(3), self.client.request(request)).await,
            Ok(Ok(response)) if response.status() == StatusCode::OK
        )
    }

    /// Manda o corpo ao `llama-server` e devolve a resposta sem esperar o fim. A cota do tenant é
    /// cobrada pelo `usage` que o próprio servidor devolve; o `permit` (a vaga única) só volta quando
    /// a resposta termina ou o cliente desiste.
    pub async fn forward(&self, body: Bytes, tenant: Arc<Tenant>, permit: Permit) -> Response {
        let streaming = serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .is_some_and(|v| v["stream"].as_bool().unwrap_or(false));
        let body = if streaming { with_usage(&body) } else { body };
        let request = Request::builder()
            .method("POST")
            .uri(format!(
                "http://127.0.0.1:{}/v1/chat/completions",
                self.port
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .expect("requisição válida");
        let response = match self.client.request(request).await {
            Ok(response) => response,
            Err(error) => {
                eprintln!("llama-server: {error}");
                return crate::error(
                    StatusCode::BAD_GATEWAY,
                    "MODEL_UNAVAILABLE",
                    "o servidor do modelo não respondeu",
                );
            }
        };
        let (parts, incoming) = response.into_parts();
        let stream = Tap {
            inner: Box::pin(Body::new(incoming).into_data_stream()),
            tail: Vec::new(),
            seen: 0,
            tenant,
            charged: false,
            _permit: permit,
        };
        let mut response = Response::from_parts(parts, Body::from_stream(stream));
        // O tamanho muda quando o corpo passa pelo gateway.
        response.headers_mut().remove(header::CONTENT_LENGTH);
        response
    }
}

/// Garante `stream_options.include_usage` no corpo de uma requisição em stream, para a cota.
fn with_usage(body: &Bytes) -> Bytes {
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return body.clone();
    };
    if let Some(object) = value.as_object_mut() {
        let options = object
            .entry("stream_options")
            .or_insert_with(|| serde_json::json!({}));
        if let Some(options) = options.as_object_mut() {
            options.insert("include_usage".into(), true.into());
        }
    }
    serde_json::to_vec(&value).map_or_else(|_| body.clone(), Bytes::from)
}

/// `total_tokens` do último `usage` num texto (o fim de um stream, ou uma resposta inteira).
pub fn total_tokens(text: &[u8]) -> Option<u64> {
    let text = std::str::from_utf8(text).ok()?;
    let at = text.rfind("\"total_tokens\"")?;
    let rest = text[at + "\"total_tokens\"".len()..]
        .trim_start_matches([' ', ':'])
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    rest.parse().ok()
}

/// Repassa a resposta e, no fim, cobra os tokens do tenant. Segura a vaga até lá.
struct Tap {
    inner: Pin<Box<dyn Stream<Item = Result<Bytes, axum::Error>> + Send>>,
    /// Últimos bytes vistos: o `usage` de um stream vem no fim.
    tail: Vec<u8>,
    /// Bytes repassados: se o cliente desiste antes do `usage`, a cota paga uma estimativa disto.
    seen: u64,
    tenant: Arc<Tenant>,
    charged: bool,
    _permit: Permit,
}

const TAIL: usize = 4096;

impl Tap {
    /// Cobra uma vez: o `usage` do fim da resposta, ou, se ela foi cortada, cerca de um token a cada
    /// quatro bytes já enviados (desistir no meio não sai de graça).
    fn charge(&mut self) {
        if self.charged {
            return;
        }
        self.charged = true;
        let tokens = total_tokens(&self.tail).unwrap_or(self.seen.div_ceil(4));
        if tokens > 0 {
            self.tenant.charge(tokens);
        }
    }
}

impl Drop for Tap {
    fn drop(&mut self) {
        self.charge();
    }
}

impl Stream for Tap {
    type Item = Result<Bytes, axum::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        match this.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                this.tail.extend_from_slice(&chunk);
                if this.tail.len() > TAIL * 2 {
                    let cut = this.tail.len() - TAIL;
                    this.tail.drain(..cut);
                }
                this.seen += chunk.len() as u64;
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(None) => {
                this.charge();
                Poll::Ready(None)
            }
            other => other,
        }
    }
}

/// Sobe os túneis, o `llama-server` e o supervisiona. Devolve já, com o estado `Loading`; o
/// carregamento e o benchmark seguem em segundo plano. `shutdown` encerra tudo.
pub fn start(config: LlamaConfig, generation: u64) -> Result<Arc<Llama>, String> {
    let llama = Arc::new(Llama {
        id: config.id.clone(),
        context: config.context,
        port: config.port,
        state: Mutex::new(State::Loading),
        tokens_per_second: Mutex::new(0.0),
        client: Client::builder(TokioExecutor::new()).build_http(),
        tasks: Mutex::new(Vec::new()),
        generation,
    });
    // Tudo o que pode falhar vem antes de qualquer tarefa: um erro no meio não deixa túnel para trás
    // segurando uma porta (os listeners soltos se fecham sozinhos).
    let mut listeners = Vec::new();
    let mut tls = None;
    if !config.endpoints.is_empty() {
        tls = Some(ai_cluster_tunnel::client_config(
            &config.ca,
            &config.cert,
            &config.key,
        )?);
        for local in config.local_endpoints()? {
            // Escuta já, de forma síncrona: o llama-server só sobe depois que as portas existem.
            let listener = std::net::TcpListener::bind(&local)
                .and_then(|l| l.set_nonblocking(true).map(|()| l))
                .and_then(tokio::net::TcpListener::from_std)
                .map_err(|e| format!("túnel em {local}: {e}"))?;
            listeners.push(listener);
        }
    }
    let mut tasks = Vec::new();
    if let Some(tls) = tls {
        for (remote, listener) in config.endpoints.iter().zip(listeners) {
            let (remote, name, tls) = (remote.clone(), config.server_name.clone(), tls.clone());
            tasks.push(tokio::spawn(async move {
                if let Err(error) =
                    ai_cluster_tunnel::forward(listener, remote.clone(), name, tls).await
                {
                    eprintln!("túnel para {remote} encerrou: {error}");
                }
            }));
        }
    }
    let background = llama.clone();
    tasks.push(tokio::spawn(async move {
        supervise(&config, &background).await;
    }));
    *llama
        .tasks
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = tasks;
    Ok(llama)
}

/// Aborta a tarefa quando sai de escopo.
struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Espera a porta local ficar livre (o `llama-server` antigo ainda pode estar saindo).
async fn wait_port_free(port: u16) {
    for _ in 0..50 {
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Roda o `llama-server`; se sair, marca falha e o sobe de novo (com espera crescente).
async fn supervise(config: &LlamaConfig, llama: &Arc<Llama>) {
    let mut wait = Duration::from_secs(2);
    loop {
        llama.set(State::Loading);
        let started = Instant::now();
        let child = tokio::process::Command::new(&config.server_bin)
            .args(config.server_args())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn();
        match child {
            Ok(mut child) => {
                let watcher = llama.clone();
                // Se o supervisor for abortado dentro do `wait`, o guarda também derruba o vigia: ele
                // não pode continuar sondando a porta de um modelo novo.
                let _ready = AbortOnDrop(tokio::spawn(async move { become_ready(&watcher).await }));
                let status = child.wait().await;
                let reason = match status {
                    Ok(status) => format!("llama-server saiu ({status})"),
                    Err(error) => format!("llama-server: {error}"),
                };
                eprintln!("{reason}; reiniciando");
                llama.set(State::Failed(reason));
            }
            Err(error) => {
                let reason = format!("llama-server não iniciou ({}): {error}", config.server_bin);
                eprintln!("{reason}");
                llama.set(State::Failed(reason));
            }
        }
        wait = if started.elapsed() > Duration::from_secs(120) {
            Duration::from_secs(2)
        } else {
            (wait * 2).min(Duration::from_secs(60))
        };
        tokio::time::sleep(wait).await;
    }
}

/// Espera o `/health` do `llama-server`, marca `Ready` e mede o benchmark.
async fn become_ready(llama: &Arc<Llama>) {
    let deadline = Instant::now() + LOAD_TIMEOUT;
    while Instant::now() < deadline {
        if llama.healthy().await {
            llama.set(State::Ready);
            match benchmark(llama).await {
                Some(rate) => {
                    *llama
                        .tokens_per_second
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) = rate;
                    println!(
                        "modelo {} pronto: {rate:.2} tokens/s no benchmark",
                        llama.id
                    );
                }
                None => println!("modelo {} pronto (benchmark sem medida)", llama.id),
            }
            return;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    llama.set(State::Failed("o modelo não ficou pronto a tempo".into()));
}

/// Um pedido fixo; lê `timings.predicted_per_second` da resposta do `llama-server`.
async fn benchmark(llama: &Llama) -> Option<f64> {
    let body = serde_json::json!({
        "model": llama.id,
        "messages": [{"role": "user", "content": BENCH_PROMPT}],
        "max_tokens": BENCH_TOKENS,
        "temperature": 0,
    });
    let request = Request::builder()
        .method("POST")
        .uri(format!(
            "http://127.0.0.1:{}/v1/chat/completions",
            llama.port
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .ok()?;
    let response = tokio::time::timeout(Duration::from_secs(900), llama.client.request(request))
        .await
        .ok()?
        .ok()?;
    let bytes = axum::body::to_bytes(Body::new(response.into_body()), 1 << 20)
        .await
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value["timings"]["predicted_per_second"]
        .as_f64()
        .filter(|rate| rate.is_finite() && *rate > 0.0)
}

/// O modelo em execução e o que o controller pediu (ADR-022): o gateway aplica o `DesiredModel` que
/// recebe a cada apresentação. Uma configuração do ambiente (`AI_CLUSTER_LLAMA_MODEL`) vale mais que
/// o controller e fica fixa.
pub struct Manager {
    current: Mutex<Option<Arc<Llama>>>,
    base: Base,
    /// Com modelo fixo pelo ambiente, o plano do controller é ignorado.
    fixed: bool,
    /// O último plano recusado, para o log não repetir o mesmo erro a cada apresentação.
    last_error: Mutex<String>,
}

/// O que não muda com o modelo: binário, pasta de modelos, TLS e portas locais.
#[derive(Debug, Clone, Default)]
pub struct Base {
    pub server_bin: String,
    pub models_dir: String,
    pub ca: String,
    pub cert: String,
    pub key: String,
    pub server_name: String,
    pub port: u16,
    pub tunnel_base: u16,
    pub default_context: u32,
    pub slots: u32,
}

impl Base {
    pub fn from_env() -> Result<Self, String> {
        let var = |name: &str| {
            std::env::var(name)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let number = |name: &str, default: u32| -> Result<u32, String> {
            match var(name) {
                Some(text) => text.parse().map_err(|_| format!("{name} inválido: {text}")),
                None => Ok(default),
            }
        };
        let port = u16::try_from(number("AI_CLUSTER_LLAMA_PORT", u32::from(DEFAULT_PORT))?)
            .map_err(|_| "AI_CLUSTER_LLAMA_PORT fora de 0..65535".to_string())?;
        let tunnel_base = u16::try_from(number(
            "AI_CLUSTER_LLAMA_TUNNEL_BASE",
            u32::from(DEFAULT_TUNNEL_BASE),
        )?)
        .map_err(|_| "AI_CLUSTER_LLAMA_TUNNEL_BASE fora de 0..65535".to_string())?;
        Ok(Self {
            server_bin: var("AI_CLUSTER_LLAMA_SERVER_BIN").unwrap_or_else(|| "llama-server".into()),
            models_dir: var("AI_CLUSTER_MODELS_DIR").unwrap_or_default(),
            ca: var("AI_CLUSTER_LLAMA_TLS_CA").unwrap_or_default(),
            cert: var("AI_CLUSTER_LLAMA_TLS_CERT").unwrap_or_default(),
            key: var("AI_CLUSTER_LLAMA_TLS_KEY").unwrap_or_default(),
            server_name: var("AI_CLUSTER_LLAMA_RPC_SERVER_NAME")
                .unwrap_or_else(|| "aic-worker".into()),
            port,
            tunnel_base,
            default_context: number("AI_CLUSTER_LLAMA_CTX", 4096)?,
            slots: slots_from_env()?,
        })
    }

    /// A configuração de um pedido do controller. Recusa caminho que sai da pasta de modelos.
    pub fn config_for(&self, wanted: &DesiredModel) -> Result<LlamaConfig, String> {
        if self.models_dir.is_empty() {
            return Err("o gateway não tem AI_CLUSTER_MODELS_DIR".into());
        }
        let path = std::path::Path::new(&wanted.model_path);
        if wanted.model_path.is_empty()
            || path.is_absolute()
            || path
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(format!("caminho de modelo inválido: {}", wanted.model_path));
        }
        let id = &wanted.model_id;
        if id.is_empty()
            || id.len() > 64
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
        {
            return Err(format!("id de modelo inválido: {id}"));
        }
        if !wanted.endpoints.is_empty()
            && (self.ca.is_empty() || self.cert.is_empty() || self.key.is_empty())
        {
            return Err("sem AI_CLUSTER_LLAMA_TLS_*: o RPC não vai em texto puro".into());
        }
        if wanted.endpoints.len() > 64 {
            return Err("workers demais no plano".into());
        }
        // O plano vem da rede: nada de contexto que estoure a RAM nem faixa de portas fora de 16 bits.
        let context = if wanted.context == 0 {
            self.default_context
        } else {
            u32::try_from(wanted.context)
                .unwrap_or(u32::MAX)
                .min(MAX_CONTEXT)
        };
        Ok(LlamaConfig {
            id: id.clone(),
            model: std::path::Path::new(&self.models_dir)
                .join(path)
                .to_string_lossy()
                .into_owned(),
            server_bin: self.server_bin.clone(),
            endpoints: wanted.endpoints.iter().map(|e| e.address.clone()).collect(),
            ca: self.ca.clone(),
            cert: self.cert.clone(),
            key: self.key.clone(),
            server_name: self.server_name.clone(),
            context,
            gpu_layers: wanted.gpu_layers,
            port: self.port,
            tunnel_base: self.tunnel_base,
            slots: self.slots.max(1),
        })
    }
}

impl Manager {
    pub fn new(base: Base) -> Self {
        Self {
            current: Mutex::new(None),
            base,
            fixed: false,
            last_error: Mutex::new(String::new()),
        }
    }

    /// Modelo fixo pelo ambiente (`AI_CLUSTER_LLAMA_MODEL`).
    pub fn fixed(base: Base, config: LlamaConfig) -> Result<Self, String> {
        let llama = start(config, 0)?;
        Ok(Self {
            current: Mutex::new(Some(llama)),
            base,
            fixed: true,
            last_error: Mutex::new(String::new()),
        })
    }

    /// Quantas gerações ao mesmo tempo o llama-server aceita; o gateway abre essa fila.
    pub fn slots(&self) -> u32 {
        self.base.slots.max(1)
    }

    pub fn current(&self) -> Option<Arc<Llama>> {
        self.current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Estado para o controller: a geração aplicada e como o modelo está.
    pub fn status(&self) -> Option<ModelStatus> {
        let llama = self.current()?;
        let (state, reason) = match llama.state() {
            State::Loading => (ModelState::ModelLoading, String::new()),
            State::Ready => (ModelState::ModelReady, String::new()),
            State::Failed(reason) => (ModelState::ModelFailed, reason),
        };
        Some(ModelStatus {
            model_id: llama.id.clone(),
            state: state as i32,
            reason,
            generation: llama.generation,
            tokens_per_second: llama.tokens_per_second(),
        })
    }

    /// Aplica o plano do controller: `None` para o modelo, uma geração nova o troca, a mesma não faz
    /// nada. Um plano inválido deixa o modelo atual como está e é registrado.
    pub async fn apply(&self, desired: Option<&DesiredModel>) {
        if self.fixed {
            return;
        }
        let same = match (self.current(), desired) {
            (Some(now), Some(wanted)) => now.generation == wanted.generation,
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        let config = match desired.map(|wanted| self.base.config_for(wanted)) {
            Some(Err(error)) => {
                let mut last = self
                    .last_error
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if *last != error {
                    eprintln!("plano do controller recusado: {error}");
                    *last = error;
                }
                return;
            }
            Some(Ok(config)) => Some(config),
            None => None,
        };
        // O processo antigo sai antes: as portas locais são as mesmas.
        if let Some(old) = self.take() {
            for task in old.shutdown() {
                let _ = task.await;
            }
            wait_port_free(self.base.port).await;
        }
        if let (Some(config), Some(wanted)) = (config, desired) {
            match start(config, wanted.generation) {
                Ok(llama) => self.put(Some(llama)),
                Err(error) => eprintln!("modelo não iniciou: {error}"),
            }
        }
    }

    fn take(&self) -> Option<Arc<Llama>> {
        self.current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
    }

    fn put(&self, llama: Option<Arc<Llama>>) {
        *self
            .current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = llama;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_usage_of_a_stream_is_read_from_its_last_chunk() {
        let tail = br#"data: {"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":30,"total_tokens":42}}

data: [DONE]
"#;
        assert_eq!(total_tokens(tail), Some(42));
        assert_eq!(total_tokens(b"sem usage"), None);
        assert_eq!(total_tokens(br#"{"total_tokens": 7}"#), Some(7));
    }

    #[test]
    fn a_stream_request_always_asks_for_usage() {
        let body = Bytes::from(r#"{"model":"m","stream":true,"messages":[]}"#);
        let patched: serde_json::Value = serde_json::from_slice(&with_usage(&body)).unwrap();
        assert_eq!(patched["stream_options"]["include_usage"], true);
        assert_eq!(patched["model"], "m");
    }

    fn config() -> LlamaConfig {
        LlamaConfig {
            id: "m".into(),
            model: "/models/m.gguf".into(),
            server_bin: "llama-server".into(),
            endpoints: vec!["10.0.0.5:50061".into(), "10.0.0.6:50061".into()],
            ca: "ca".into(),
            cert: "c".into(),
            key: "k".into(),
            server_name: "aic-worker".into(),
            context: 2048,
            gpu_layers: 99,
            port: 18080,
            tunnel_base: 18100,
            slots: 1,
        }
    }

    #[test]
    fn the_server_only_sees_loopback_tunnels() {
        let args = config().server_args().join(" ");
        assert!(args.contains("--host 127.0.0.1"), "{args}");
        assert!(
            args.contains("--rpc 127.0.0.1:18100,127.0.0.1:18101"),
            "{args}"
        );
        assert!(
            !args.contains("10.0.0."),
            "o worker nunca aparece direto: {args}"
        );
        assert!(args.contains("-np 1"));
        let mut local = config();
        local.endpoints.clear();
        assert!(!local.server_args().join(" ").contains("--rpc"));
    }

    // O ambiente é do processo: um teste só, em série, mexe nele.
    #[test]
    fn the_environment_builds_the_config_and_refuses_clear_text_rpc() {
        let names = [
            "AI_CLUSTER_LLAMA_MODEL",
            "AI_CLUSTER_LLAMA_MODEL_ID",
            "AI_CLUSTER_LLAMA_RPC_ENDPOINTS",
            "AI_CLUSTER_LLAMA_TLS_CA",
            "AI_CLUSTER_LLAMA_TLS_CERT",
            "AI_CLUSTER_LLAMA_TLS_KEY",
            "AI_CLUSTER_LLAMA_CTX",
        ];
        let clear = || {
            names
                .iter()
                .for_each(|n| unsafe { std::env::remove_var(n) })
        };
        clear();
        assert_eq!(from_env().unwrap(), None);

        unsafe { std::env::set_var("AI_CLUSTER_LLAMA_MODEL", "/m/SmolLM2-360M-Q4.gguf") };
        let local = from_env().unwrap().unwrap();
        assert_eq!(
            (local.id.as_str(), local.context),
            ("SmolLM2-360M-Q4", 4096)
        );
        assert!(local.endpoints.is_empty());

        unsafe {
            std::env::set_var("AI_CLUSTER_LLAMA_RPC_ENDPOINTS", "pi:50061, pc:50061");
        }
        let refused = from_env().unwrap_err();
        assert!(refused.contains("texto puro"), "{refused}");

        unsafe {
            std::env::set_var("AI_CLUSTER_LLAMA_TLS_CA", "ca");
            std::env::set_var("AI_CLUSTER_LLAMA_TLS_CERT", "c");
            std::env::set_var("AI_CLUSTER_LLAMA_TLS_KEY", "k");
        }
        assert_eq!(from_env().unwrap().unwrap().endpoints.len(), 2);

        unsafe { std::env::set_var("AI_CLUSTER_LLAMA_MODEL_ID", "id com espaço") };
        assert!(from_env().is_err());
        unsafe {
            std::env::set_var("AI_CLUSTER_LLAMA_MODEL_ID", "ok");
            std::env::set_var("AI_CLUSTER_LLAMA_RPC_ENDPOINTS", "sem-porta");
        }
        assert!(from_env().is_err());
        clear();
    }
    fn desired(path: &str, id: &str, endpoints: &[&str]) -> DesiredModel {
        DesiredModel {
            generation: 3,
            model_id: id.into(),
            model_path: path.into(),
            context: 0,
            gpu_layers: 99,
            endpoints: endpoints
                .iter()
                .map(|a| ai_cluster_protocol::cluster::RpcEndpoint {
                    node_id: "n".into(),
                    address: a.to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn the_controller_plan_cannot_leave_the_models_directory() {
        let base = Base {
            server_bin: "llama-server".into(),
            models_dir: "/models".into(),
            ca: "ca".into(),
            cert: "c".into(),
            key: "k".into(),
            server_name: "aic-worker".into(),
            port: 18080,
            tunnel_base: 18100,
            default_context: 4096,
            slots: 1,
        };
        let ok = base
            .config_for(&desired("pasta/m.gguf", "pasta-m", &["10.0.0.5:50061"]))
            .unwrap();
        assert_eq!(ok.context, 4096);
        assert!(ok.model.ends_with("m.gguf") && ok.model.starts_with("/models"));
        for bad in ["../fora.gguf", "/etc/passwd", "a/../../b.gguf", ""] {
            assert!(base.config_for(&desired(bad, "m", &[])).is_err(), "{bad}");
        }
        assert!(base.config_for(&desired("m.gguf", "id ruim", &[])).is_err());
        // Workers sem TLS configurado: recusa em vez de falar em texto puro.
        let mut open = base.clone();
        open.ca.clear();
        assert!(
            open.config_for(&desired("m.gguf", "m", &["10.0.0.5:50061"]))
                .is_err()
        );
        assert!(open.config_for(&desired("m.gguf", "m", &[])).is_ok());
    }
    #[test]
    fn the_tunnel_range_cannot_overflow_or_cross_the_server_port() {
        let mut c = config();
        c.tunnel_base = u16::MAX - 1; // cabem 2 portas: 65534 e 65535
        assert_eq!(c.local_endpoints().unwrap().len(), 2);
        c.endpoints.push("10.0.0.7:50061".into());
        assert!(
            c.local_endpoints().is_err(),
            "a terceira porta passaria de 65535"
        );
        let mut clash = config();
        clash.port = clash.tunnel_base + 1; // o llama-server no meio da faixa
        assert!(clash.local_endpoints().is_err());
    }

    #[tokio::test]
    async fn a_client_that_leaves_early_still_pays_an_estimate() {
        use crate::request_manager::{Admission, Class};
        let tenant = Arc::new(Tenant::new("t", "k"));
        let admission = Admission::new(1, Duration::from_secs(1));
        let permit = admission.acquire(Class::Normal).await.unwrap();
        let chunks: Vec<Result<Bytes, axum::Error>> = vec![
            Ok(Bytes::from(vec![b'x'; 400])),
            Ok(Bytes::from(vec![b'y'; 400])),
        ];
        let mut tap = Tap {
            inner: Box::pin(futures_core_stream(chunks)),
            tail: Vec::new(),
            seen: 0,
            tenant: tenant.clone(),
            charged: false,
            _permit: permit,
        };
        // Lê só o primeiro pedaço e desiste.
        let first = std::future::poll_fn(|cx| Pin::new(&mut tap).poll_next(cx)).await;
        assert!(first.is_some());
        drop(tap);
        assert_eq!(tenant.charged_total(), 100, "400 bytes ≈ 100 tokens");
    }

    /// Um stream pronto, sem depender de uma biblioteca de streams nos testes.
    fn futures_core_stream(
        items: Vec<Result<Bytes, axum::Error>>,
    ) -> impl Stream<Item = Result<Bytes, axum::Error>> + Send {
        struct Ready(std::vec::IntoIter<Result<Bytes, axum::Error>>);
        impl Stream for Ready {
            type Item = Result<Bytes, axum::Error>;
            fn poll_next(
                mut self: Pin<&mut Self>,
                _: &mut Context<'_>,
            ) -> Poll<Option<Self::Item>> {
                Poll::Ready(self.0.next())
            }
        }
        Ready(items.into_iter())
    }
    #[test]
    fn the_slots_set_np_and_scale_the_total_context() {
        let mut c = config();
        c.slots = 4;
        c.context = 2048;
        let args = c.server_args().join(" ");
        assert!(args.contains("-np 4"), "{args}");
        assert!(args.contains("-c 8192"), "cada vaga tem 2048: {args}");
        assert!(config().server_args().join(" ").contains("-np 1"));
    }
}
