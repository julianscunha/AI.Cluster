//! Reservas de nós para execução futura (G6).
//!
//! Uma reserva retém um nó para um `request_id` **antes** de o trabalho existir. O valor
//! está em fixar afinidade: um cliente que vai enfileirar um pedido logo reserva o nó onde
//! os dados dele já estão, e enquanto a reserva vale nenhuma outra requisição pode usar
//! esse nó. Sem reserva, o nó ficaria disponível e qualquer requisição o tomaria.
//!
//! A reserva não empurra trabalho — quem puxa é o worker. Ela só restringe quem *pode*
//! receber um nó: a requisição dona, quando seu worker fizer poll. Um cliente que reserva e
//! nunca envia nada deixa de segurar o nó quando o TTL vence.
//!
//! ponytail: a reserva guarda o nó inteiro, não uma fração da capacidade dele. Reservar
//! "metade dos cores" só faz sentido junto de um modelo de capacidade por nó, que o
//! placement ainda não tem (ele só enxerga ocioso ou com uma unidade em voo).

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

/// TTL padrão de uma reserva quando o cliente não informa um (60s).
pub const DEFAULT_TTL_MS: u64 = 60_000;

#[derive(Clone, Debug, PartialEq)]
pub struct Reservation {
    pub request_id: String,
    pub expires_at_unix_ms: u64,
}

#[derive(Debug, Default)]
pub struct Reservations {
    held: Mutex<HashMap<String, Reservation>>,
}

impl Reservations {
    pub fn new() -> Self {
        Self::default()
    }

    /// Retém um nó para um `request_id` por `ttl_ms` a partir de `now`.
    ///
    /// Uma reserva nova sobre o mesmo nó substitui a anterior: um nó, uma reserva. TTL
    /// zero usa o padrão; um TTL explícito é respeitado como veio.
    pub fn reserve(&self, node_id: &str, request_id: &str, ttl_ms: u64, now: u64) -> Reservation {
        let ttl = if ttl_ms == 0 { DEFAULT_TTL_MS } else { ttl_ms };
        let reservation = Reservation {
            request_id: request_id.to_string(),
            expires_at_unix_ms: now.saturating_add(ttl),
        };
        self.lock().insert(node_id.to_string(), reservation.clone());
        reservation
    }

    /// Libera a reserva de um nó. Devolve false quando não havia reserva ativa.
    pub fn release(&self, node_id: &str) -> bool {
        self.lock().remove(node_id).is_some()
    }

    /// A requisição para quem o nó está retido agora, ou None quando livre ou expirado.
    ///
    /// Uma reserva vencida é descartada na leitura, então o nó volta a ser candidato sem
    /// nenhum timer de fundo.
    pub fn held_for(&self, node_id: &str, now: u64) -> Option<String> {
        let mut held = self.lock();
        match held.get(node_id) {
            Some(reservation) if reservation.expires_at_unix_ms > now => {
                Some(reservation.request_id.clone())
            }
            Some(_) => {
                held.remove(node_id);
                None
            }
            None => None,
        }
    }

    /// Reservas ativas, ordenadas por nó. As vencidas são descartadas na leitura.
    pub fn list(&self, now: u64) -> Vec<(String, Reservation)> {
        let mut held = self.lock();
        held.retain(|_, reservation| reservation.expires_at_unix_ms > now);
        let mut ids: Vec<&String> = held.keys().collect();
        ids.sort();
        ids.into_iter()
            .map(|id| (id.clone(), held[id].clone()))
            .collect()
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Reservation>> {
        self.held
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reserved_node_is_held_for_its_request() {
        let reservations = Reservations::new();
        let held = reservations.reserve("node-a", "req-x", 5_000, 1_000);

        assert_eq!(held.expires_at_unix_ms, 6_000);
        assert_eq!(
            reservations.held_for("node-a", 2_000),
            Some("req-x".to_string())
        );
        // A node nobody reserved is free.
        assert_eq!(reservations.held_for("node-b", 2_000), None);
    }

    #[test]
    fn a_zero_ttl_falls_back_to_the_default() {
        let reservations = Reservations::new();
        let held = reservations.reserve("node-a", "req-x", 0, 1_000);
        assert_eq!(held.expires_at_unix_ms, 1_000 + DEFAULT_TTL_MS);
    }

    #[test]
    fn an_expired_reservation_frees_the_node_on_read() {
        let reservations = Reservations::new();
        reservations.reserve("node-a", "req-x", 5_000, 1_000);

        // Still inside the window.
        assert_eq!(
            reservations.held_for("node-a", 5_999),
            Some("req-x".to_string())
        );
        // Past it: the hold is gone, and the entry is dropped so it cannot come back.
        assert_eq!(reservations.held_for("node-a", 6_000), None);
        assert_eq!(reservations.list(6_000).len(), 0);
    }

    #[test]
    fn release_frees_a_held_node_and_reports_whether_it_did() {
        let reservations = Reservations::new();
        reservations.reserve("node-a", "req-x", 5_000, 1_000);

        assert!(reservations.release("node-a"));
        assert_eq!(reservations.held_for("node-a", 2_000), None);
        // Releasing a node nobody held is not an error.
        assert!(!reservations.release("node-a"));
    }

    #[test]
    fn a_second_reservation_replaces_the_first() {
        let reservations = Reservations::new();
        reservations.reserve("node-a", "req-x", 5_000, 1_000);
        reservations.reserve("node-a", "req-y", 9_000, 1_000);

        assert_eq!(
            reservations.held_for("node-a", 2_000),
            Some("req-y".to_string())
        );
        assert_eq!(reservations.list(2_000).len(), 1);
    }

    #[test]
    fn list_drops_expired_holds_and_sorts_by_node() {
        let reservations = Reservations::new();
        reservations.reserve("node-b", "req-x", 5_000, 1_000);
        reservations.reserve("node-a", "req-y", 1_000, 1_000);

        let listed = reservations.list(2_000);
        // node-a's reservation expired at 2_000, so only node-b remains.
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].0, "node-b");
    }
}
