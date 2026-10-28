//! Request Manager (F11), the part that lives in the gateway: who is asking (tenants), what they
//! may use (model ACL, token quota), and in what order they are served (a priority queue in front
//! of the single generation slot).
//!
//! It is the first step of `request-manager`: identity, quota and ordering. Request states,
//! idempotency and fairness across tenants are not here.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ai_cluster_protocol::metadata::same_token;
use axum::http::{HeaderMap, header};
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;

/// Priority classes of the API (`inference-api`), lowest first. A higher class is served first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    Background = 0,
    Batch = 1,
    Normal = 2,
    Interactive = 3,
    Realtime = 4,
}

impl Class {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "background" => Self::Background,
            "batch" => Self::Batch,
            "normal" => Self::Normal,
            "interactive" => Self::Interactive,
            "realtime" => Self::Realtime,
            _ => return None,
        })
    }
}

/// One application of the API: a key, the models it may call, a token quota and the highest class
/// it may ask for.
#[derive(Debug)]
pub struct Tenant {
    pub name: String,
    key: String,
    /// `None` is every model.
    models: Option<Vec<String>>,
    /// Tokens (prompt plus generated) per window; `None` is unlimited.
    quota: Option<u64>,
    pub ceiling: Class,
    window: Mutex<(Instant, u64)>,
    tokens_total: AtomicU64,
    rejected_total: AtomicU64,
}

impl Tenant {
    pub fn new(name: &str, key: &str) -> Self {
        Self {
            name: name.to_string(),
            key: key.to_string(),
            models: None,
            quota: None,
            ceiling: Class::Realtime,
            window: Mutex::new((Instant::now(), 0)),
            tokens_total: AtomicU64::new(0),
            rejected_total: AtomicU64::new(0),
        }
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn allows(&self, model: &str) -> bool {
        self.models
            .as_ref()
            .is_none_or(|allowed| allowed.iter().any(|name| name == model))
    }

    /// The class a request runs in: what it asked for, never above the tenant's ceiling. Without
    /// a request, `Normal`, or the ceiling if that is lower.
    pub fn class_for(&self, asked: Option<Class>) -> Class {
        asked.unwrap_or(Class::Normal).min(self.ceiling)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, (Instant, u64)> {
        self.window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// `Err(seconds)` when the quota of the current window is spent. The window restarts on its
    /// own once `window` has passed since it began.
    pub fn check_quota(&self, window: Duration) -> Result<(), u64> {
        let Some(quota) = self.quota else {
            return Ok(());
        };
        let mut guard = self.lock();
        if guard.0.elapsed() >= window {
            *guard = (Instant::now(), 0);
        }
        if guard.1 >= quota {
            self.rejected_total.fetch_add(1, Ordering::Relaxed);
            return Err(window.saturating_sub(guard.0.elapsed()).as_secs().max(1));
        }
        Ok(())
    }

    /// Counts what a finished request used. The quota is checked before a request starts, so the
    /// last request of a window may overshoot it by its own size.
    pub fn charge(&self, tokens: u64) {
        self.tokens_total.fetch_add(tokens, Ordering::Relaxed);
        self.lock().1 += tokens;
    }

    /// Tokens cobrados ao tenant desde o início.
    #[cfg(test)]
    pub fn charged_total(&self) -> u64 {
        self.tokens_total.load(Ordering::Relaxed)
    }

    /// A rejection that is not the quota's (capacity), counted for the tenant.
    pub fn note_rejected(&self) {
        self.rejected_total.fetch_add(1, Ordering::Relaxed);
    }
}

/// All tenants of the gateway.
#[derive(Debug)]
pub struct Tenants {
    list: Vec<Arc<Tenant>>,
    /// Chaves que o controller entrega (só o hash): entram e saem a cada apresentação.
    granted: Mutex<Vec<([u8; 32], Arc<Tenant>)>>,
    pub window: Duration,
}

/// Uma chave de API do controller: o tenant, o `sha256` da chave e os modelos que ela alcança.
pub struct Grant {
    pub name: String,
    pub sha256: [u8; 32],
    /// `None` = todos os modelos.
    pub models: Option<Vec<String>>,
}

/// Parses `name=key[,models=a|b][,tokens=N][,class=batch]` entries separated by `;`.
pub fn parse_tenants(spec: &str) -> Result<Vec<Tenant>, String> {
    let mut tenants = Vec::new();
    for entry in spec.split(';').map(str::trim).filter(|e| !e.is_empty()) {
        let mut fields = entry.split(',').map(str::trim);
        let (name, key) = fields
            .next()
            .and_then(|first| first.split_once('='))
            .filter(|(name, key)| !name.is_empty() && !key.is_empty())
            .ok_or_else(|| format!("tenant '{entry}': esperado nome=chave"))?;
        // The name is a Prometheus label value: letters, digits, '_' and '-' keep it harmless.
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(format!(
                "tenant '{name}': use só letras, dígitos, '_' e '-' no nome"
            ));
        }
        let mut tenant = Tenant::new(name, key);
        for field in fields {
            let (what, value) = field
                .split_once('=')
                .ok_or_else(|| format!("tenant '{name}': campo '{field}' sem '='"))?;
            match what {
                "models" => {
                    tenant.models = Some(value.split('|').map(str::to_string).collect());
                }
                "tokens" => {
                    tenant.quota = Some(
                        value
                            .parse()
                            .map_err(|_| format!("tenant '{name}': tokens inválido '{value}'"))?,
                    );
                }
                "class" => {
                    tenant.ceiling = Class::parse(value)
                        .ok_or_else(|| format!("tenant '{name}': classe inválida '{value}'"))?;
                }
                other => return Err(format!("tenant '{name}': campo desconhecido '{other}'")),
            }
        }
        if tenants
            .iter()
            .any(|known: &Tenant| known.name == tenant.name)
        {
            return Err(format!("tenant '{name}' repetido"));
        }
        tenants.push(tenant);
    }
    Ok(tenants)
}

impl Tenants {
    pub fn new(list: Vec<Tenant>, window: Duration) -> Self {
        Self {
            list: list.into_iter().map(Arc::new).collect(),
            granted: Mutex::new(Vec::new()),
            window,
        }
    }

    /// Troca as chaves do controller. Quem continua com o mesmo nome e hash mantém o tenant (e os
    /// contadores); o resto sai.
    pub fn set_grants(&self, grants: Vec<Grant>) {
        let mut granted = self
            .granted
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut next = Vec::with_capacity(grants.len());
        for grant in grants {
            let kept = granted
                .iter()
                .find(|(sha, tenant)| *sha == grant.sha256 && tenant.name == grant.name)
                .map(|(_, tenant)| tenant.clone());
            let tenant = kept.unwrap_or_else(|| {
                let mut tenant = Tenant::new(&grant.name, "");
                tenant.models = grant.models.clone();
                Arc::new(tenant)
            });
            next.push((grant.sha256, tenant));
        }
        *granted = next;
    }

    /// The tenant whose key the call carries as `Authorization: Bearer`. Every key is compared,
    /// so the time taken does not say which one matched.
    pub fn authenticate(&self, headers: &HeaderMap) -> Option<Arc<Tenant>> {
        let given = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .unwrap_or("");
        let mut found = None;
        for tenant in &self.list {
            if !tenant.key.is_empty()
                && same_token(tenant.key.as_bytes(), given.as_bytes())
                && found.is_none()
            {
                found = Some(tenant.clone());
            }
        }
        // As chaves do controller chegam em hash: confere o hash do que o cliente mandou, de todas.
        if !given.is_empty() {
            let hash: [u8; 32] = Sha256::digest(given.as_bytes()).into();
            let granted = self
                .granted
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for (sha, tenant) in granted.iter() {
                if same_token(sha, &hash) && found.is_none() {
                    found = Some(tenant.clone());
                }
            }
        }
        found
    }

    /// The calling tenant's counters in Prometheus text (labels are escaped).
    pub fn render(&self, only: &Tenant) -> String {
        let mut out = String::new();
        for (name, kind, help) in [
            (
                "ai_cluster_tenant_tokens_total",
                "counter",
                "Tokens (prompt and generated) charged to the tenant.",
            ),
            (
                "ai_cluster_tenant_rejected_total",
                "counter",
                "Requests refused for quota or capacity.",
            ),
        ] {
            out.push_str(&format!("# HELP {name} {help}\n# TYPE {name} {kind}\n"));
            // A tenant sees its own counters, not the others'.
            let granted: Vec<Arc<Tenant>> = self
                .granted
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .iter()
                .map(|(_, tenant)| tenant.clone())
                .collect();
            for tenant in self
                .list
                .iter()
                .chain(granted.iter())
                .filter(|tenant| tenant.name == only.name)
            {
                let value = if name.ends_with("tokens_total") {
                    tenant.tokens_total.load(Ordering::Relaxed)
                } else {
                    tenant.rejected_total.load(Ordering::Relaxed)
                };
                out.push_str(&format!("{name}{{tenant=\"{}\"}} {value}\n", tenant.name));
            }
        }
        out
    }
}

/// Why a request did not get the slot.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// Too many already waiting.
    QueueFull,
    /// Waited as long as the gateway allows.
    TimedOut,
}

struct Waiter {
    class: Class,
    since: Instant,
    seq: u64,
    wake: oneshot::Sender<()>,
}

#[derive(Default)]
struct State {
    busy: bool,
    next_seq: u64,
    waiters: Vec<Waiter>,
}

/// One generation at a time, handed out by class: the highest waiting class first, the oldest
/// first inside a class, and a waiter moves up one class for every `AGING` it has waited, so
/// `Background` is not starved by a stream of `Interactive`.
pub struct Admission {
    state: Mutex<State>,
    max_waiting: usize,
    max_wait: Duration,
}

/// How long a waiter waits to be promoted one class.
const AGING: Duration = Duration::from_secs(5);

/// The slot. Dropping it hands the generation to the best waiter.
pub struct Permit(Arc<Admission>);

impl Admission {
    pub fn new(max_waiting: usize, max_wait: Duration) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State::default()),
            max_waiting,
            max_wait,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[cfg(test)]
    pub fn waiting(&self) -> usize {
        self.lock().waiters.len()
    }

    pub async fn acquire(self: &Arc<Self>, class: Class) -> Result<Permit, Refused> {
        let (seq, receiver) = {
            let mut state = self.lock();
            if !state.busy {
                state.busy = true;
                return Ok(Permit(self.clone()));
            }
            if state.waiters.len() >= self.max_waiting {
                return Err(Refused::QueueFull);
            }
            let (wake, receiver) = oneshot::channel();
            let seq = state.next_seq;
            state.next_seq += 1;
            state.waiters.push(Waiter {
                class,
                since: Instant::now(),
                seq,
                wake,
            });
            (seq, receiver)
        };
        // However this future ends (woken, timed out, or dropped because the client left), the
        // guard leaves the line, or passes the slot on if it had already been handed to us.
        let mut place = Place {
            admission: self.clone(),
            seq,
            woken: false,
        };
        match tokio::time::timeout(self.max_wait, receiver).await {
            Ok(Ok(())) => {
                place.woken = true;
                Ok(Permit(self.clone()))
            }
            _ => Err(Refused::TimedOut),
        }
    }

    fn release(&self) {
        let mut state = self.lock();
        let now = Instant::now();
        loop {
            // Effective class: the asked class plus one per `AGING` waited, ties to the oldest.
            let best = state
                .waiters
                .iter()
                .enumerate()
                .max_by_key(|(_, waiter)| {
                    let aged =
                        (now.duration_since(waiter.since).as_secs() / AGING.as_secs()) as usize;
                    (waiter.class as usize + aged, std::cmp::Reverse(waiter.seq))
                })
                .map(|(index, _)| index);
            let Some(index) = best else {
                state.busy = false;
                return;
            };
            let waiter = state.waiters.remove(index);
            // A waiter that timed out and left has dropped its receiver; try the next one.
            if waiter.wake.send(()).is_ok() {
                return;
            }
        }
    }
}

/// A waiter's place in line. Dropped unless it was woken, it removes the waiter, or releases the
/// slot if the wake-up reached a future that no longer exists.
struct Place {
    admission: Arc<Admission>,
    seq: u64,
    woken: bool,
}

impl Drop for Place {
    fn drop(&mut self) {
        if self.woken {
            return;
        }
        let mut state = self.admission.lock();
        let before = state.waiters.len();
        state.waiters.retain(|waiter| waiter.seq != self.seq);
        if state.waiters.len() == before {
            // Not in the list: `release` already took it and the slot is ours, unclaimed.
            drop(state);
            self.admission.release();
        }
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        self.0.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tenants_parse_with_models_quota_and_class() {
        let tenants =
            parse_tenants("acme=k1,models=gpt2|smollm2,tokens=500,class=batch; beta=k2").unwrap();
        assert_eq!(tenants.len(), 2);
        assert!(tenants[0].allows("gpt2") && !tenants[0].allows("other"));
        assert_eq!(tenants[0].quota, Some(500));
        assert_eq!(tenants[0].ceiling, Class::Batch);
        assert!(tenants[1].allows("anything") && tenants[1].quota.is_none());
        assert!(parse_tenants("sem-igual").is_err());
        assert!(parse_tenants("a=k,class=ultra").is_err());
        assert!(parse_tenants("a=k,tokens=muitos").is_err());
        assert!(parse_tenants("a=k,foo=1").is_err());
        assert!(parse_tenants("a=k;a=j").is_err());
    }

    #[test]
    fn the_class_never_passes_the_tenants_ceiling() {
        let mut tenant = Tenant::new("t", "k");
        tenant.ceiling = Class::Batch;
        assert_eq!(tenant.class_for(None), Class::Batch);
        assert_eq!(tenant.class_for(Some(Class::Realtime)), Class::Batch);
        assert_eq!(tenant.class_for(Some(Class::Background)), Class::Background);
        assert_eq!(Tenant::new("u", "k").class_for(None), Class::Normal);
    }

    #[test]
    fn the_quota_closes_the_window_and_reopens_it() {
        let mut tenant = Tenant::new("t", "k");
        tenant.quota = Some(100);
        let window = Duration::from_millis(50);
        assert!(tenant.check_quota(window).is_ok());
        tenant.charge(100);
        assert!(tenant.check_quota(window).is_err());
        assert_eq!(tenant.rejected_total.load(Ordering::Relaxed), 1);
        std::thread::sleep(Duration::from_millis(60));
        assert!(tenant.check_quota(window).is_ok());
        // Unlimited tenants are never refused.
        let free = Tenant::new("f", "k");
        free.charge(u64::MAX / 2);
        assert!(free.check_quota(window).is_ok());
    }

    #[test]
    fn only_the_exact_key_authenticates_its_tenant() {
        let tenants = Tenants::new(parse_tenants("a=ka;b=kb").unwrap(), Duration::from_secs(60));
        let with = |value: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(header::AUTHORIZATION, value.parse().unwrap());
            headers
        };
        assert_eq!(tenants.authenticate(&with("Bearer kb")).unwrap().name, "b");
        assert!(tenants.authenticate(&with("Bearer kc")).is_none());
        assert!(tenants.authenticate(&with("kb")).is_none());
        assert!(tenants.authenticate(&HeaderMap::new()).is_none());
    }

    #[tokio::test]
    async fn waiters_are_served_by_class_then_by_arrival() {
        let admission = Admission::new(8, Duration::from_secs(5));
        let first = admission.acquire(Class::Normal).await.unwrap();
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut tasks = Vec::new();
        for (name, class) in [
            ("batch", Class::Batch),
            ("interactive-1", Class::Interactive),
            ("interactive-2", Class::Interactive),
        ] {
            let (waiting_room, order) = (admission.clone(), order.clone());
            tasks.push(tokio::spawn(async move {
                let permit = waiting_room.acquire(class).await.unwrap();
                order.lock().unwrap().push(name);
                drop(permit);
            }));
            // Arrival order is the order of spawning.
            while admission.waiting() < tasks.len() {
                tokio::task::yield_now().await;
            }
        }
        drop(first);
        for task in tasks {
            task.await.unwrap();
        }
        assert_eq!(
            *order.lock().unwrap(),
            ["interactive-1", "interactive-2", "batch"]
        );
    }

    #[tokio::test]
    async fn a_client_that_leaves_the_line_frees_its_place_and_never_the_slot_for_good() {
        let admission = Admission::new(1, Duration::from_secs(5));
        let held = admission.acquire(Class::Normal).await.unwrap();
        let leaver = {
            let waiting_room = admission.clone();
            tokio::spawn(async move { waiting_room.acquire(Class::Normal).await.map(|_| ()) })
        };
        while admission.waiting() < 1 {
            tokio::task::yield_now().await;
        }
        // The client disconnects: its future is dropped while it waits.
        leaver.abort();
        let _ = leaver.await;
        assert_eq!(admission.waiting(), 0);
        // The slot was not lost with it: once released, the next request gets it at once.
        drop(held);
        assert!(admission.acquire(Class::Normal).await.is_ok());
    }

    #[tokio::test]
    async fn a_full_queue_refuses_and_a_long_wait_times_out() {
        let admission = Admission::new(1, Duration::from_millis(50));
        let held = admission.acquire(Class::Normal).await.unwrap();
        let waiter = {
            let waiting_room = admission.clone();
            tokio::spawn(async move { waiting_room.acquire(Class::Normal).await.err() })
        };
        while admission.waiting() < 1 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            admission.acquire(Class::Normal).await.err(),
            Some(Refused::QueueFull)
        );
        assert_eq!(waiter.await.unwrap(), Some(Refused::TimedOut));
        assert_eq!(admission.waiting(), 0);
        // The slot is still usable once the holder lets go.
        drop(held);
        assert!(admission.acquire(Class::Normal).await.is_ok());
    }
    #[test]
    fn controller_keys_authenticate_by_hash_and_keep_their_counters() {
        use sha2::{Digest, Sha256};
        let tenants = Tenants::new(Vec::new(), Duration::from_secs(60));
        let hash: [u8; 32] = Sha256::digest(b"aic_segredo").into();
        let grant = || Grant {
            name: "default".into(),
            sha256: hash,
            models: None,
        };
        let headers = |key: &str| {
            let mut h = HeaderMap::new();
            h.insert(
                header::AUTHORIZATION,
                format!("Bearer {key}").parse().unwrap(),
            );
            h
        };
        assert!(tenants.authenticate(&headers("aic_segredo")).is_none());
        tenants.set_grants(vec![grant()]);
        let first = tenants
            .authenticate(&headers("aic_segredo"))
            .expect("chave do controller");
        assert_eq!(first.name, "default");
        assert!(tenants.authenticate(&headers("errada")).is_none());
        assert!(tenants.authenticate(&HeaderMap::new()).is_none());
        // A mesma chave na apresentação seguinte mantém o tenant e os contadores.
        first.charge(10);
        tenants.set_grants(vec![grant()]);
        let again = tenants.authenticate(&headers("aic_segredo")).unwrap();
        assert!(Arc::ptr_eq(&first, &again));
        // Sem a chave na lista, ela deixa de valer.
        tenants.set_grants(Vec::new());
        assert!(tenants.authenticate(&headers("aic_segredo")).is_none());
    }
}
