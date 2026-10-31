//! Veredito OK/NOK de um modelo contra a RAM do cluster (ADR-022, E2): soma o que os workers aprovados e
//! vivos oferecem e compara com pesos, cache de contexto e margem. Não há teto de tamanho: um modelo
//! cabe se a soma dos workers comportar; quando não cabe, o veredito diz quanto falta.

use crate::ModelEntry;

/// Contexto usado na estimativa quando o chamador não escolhe: o menor entre o do modelo e este.
pub const DEFAULT_CONTEXT: u64 = 4096;
/// Fração da RAM de um worker que o cluster pode ocupar: o resto fica para o sistema.
const USABLE_FRACTION: f64 = 0.8;
/// Reserva fixa por worker para o processo do worker e buffers de execução.
const NODE_OVERHEAD: u64 = 128 << 20;
/// Margem sobre os pesos para buffers de cálculo e fragmentação.
const WEIGHT_MARGIN: f64 = 0.10;
/// Abaixo disto a média por worker não serve para contar quantos workers faltam (a divisão dispara).
const MIN_USEFUL_NODE: u64 = 64 << 20;

/// A RAM de um worker aprovado e vivo, como ele a reportou.
#[derive(Debug, Clone)]
pub struct NodeMemory {
    pub total_bytes: u64,
    /// Fração em uso agora (0..1), da última medida de saúde.
    pub used_fraction: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub ok: bool,
    pub weights_bytes: u64,
    pub kv_bytes: u64,
    pub margin_bytes: u64,
    pub required_bytes: u64,
    pub available_bytes: u64,
    pub nodes: u32,
    /// Ainda cabe sem o maior worker; só informa, não muda `ok` (com 2 workers em casa quase nada passaria).
    pub survives_one_loss: bool,
    /// RAM estimada do `llama-server` no gateway (0,1 GB + 30% dos pesos, medido com três modelos).
    pub gateway_bytes: u64,
    pub message: String,
}

/// RAM que um worker oferece: o menor entre o que está livre e a fração utilizável, menos a reserva.
pub fn usable_bytes(node: &NodeMemory) -> u64 {
    let total = node.total_bytes as f64;
    // Uma medida inválida (NaN, infinita) conta como worker cheio, nunca como worker livre.
    let used = if node.used_fraction.is_finite() {
        node.used_fraction.clamp(0.0, 1.0)
    } else {
        1.0
    };
    let free = total * (1.0 - used);
    let cap = total * USABLE_FRACTION;
    (free.min(cap) as u64).saturating_sub(NODE_OVERHEAD)
}

/// Cache de chave/valor em f16 para `context` tokens: 2 · camadas · contexto · largura · (kv/cabeças) · 2 B.
/// Zero quando o cabeçalho não traz as dimensões.
pub fn kv_bytes(model: &ModelEntry, context: u64) -> u64 {
    if model.layers == 0 || model.hidden == 0 || model.heads == 0 {
        return 0;
    }
    // Cabeças de chave/valor nunca passam das de consulta; zero = sem atenção agrupada.
    let kv_heads = if model.kv_heads == 0 {
        model.heads
    } else {
        model.kv_heads.min(model.heads)
    };
    let context = context.min(if model.context > 0 {
        model.context
    } else {
        context
    });
    let kv_width = u128::from(model.hidden) * u128::from(kv_heads) / u128::from(model.heads);
    let bytes = 4u128
        .checked_mul(u128::from(model.layers))
        .and_then(|b| b.checked_mul(u128::from(context)))
        .and_then(|b| b.checked_mul(kv_width));
    bytes
        .and_then(|b| u64::try_from(b).ok())
        .unwrap_or(u64::MAX)
}

fn plural(count: u32) -> String {
    if count == 1 {
        "o 1 worker aprovado e vivo oferece".to_string()
    } else {
        format!("os {count} workers aprovados e vivos oferecem")
    }
}

fn gb(bytes: u64) -> f64 {
    bytes as f64 / 1_073_741_824.0
}

/// `AI_CLUSTER_LLAMA_SLOTS` (1 a 16, padrão 1): o `llama-server` aloca o cache de cada vaga, então o
/// controller lê a mesma variável do gateway para estimar o cache total.
pub fn slots_from_env() -> u64 {
    std::env::var("AI_CLUSTER_LLAMA_SLOTS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map_or(1, |n| n.clamp(1, 16))
}

/// RAM que o `llama-server` ocupa no gateway: reserva fixa mais uma fração dos pesos. Medido (RSS em
/// repouso) com 0,25, 0,37 e 0,92 GB de pesos: 84, 210 e 262 MB. Durante a carga o pico chega a 1,3 a
/// 1,5 vezes os pesos, mas são páginas do arquivo mapeado, que o kernel devolve sob pressão.
pub fn gateway_need(weights_bytes: u64) -> u64 {
    (128 << 20) + (weights_bytes as f64 * 0.30) as u64
}

/// Compara `model` com a RAM de `nodes` para `slots` vagas de `context` tokens cada (0 = o padrão).
/// `gateway_free` é a RAM disponível que o gateway informou (`None` = não conhecida, não confere).
pub fn check(
    model: &ModelEntry,
    context: u64,
    slots: u64,
    nodes: &[NodeMemory],
    gateway_free: Option<u64>,
) -> Verdict {
    let context = if context == 0 {
        DEFAULT_CONTEXT
    } else {
        context
    };
    let weights_bytes = model.bytes;
    let kv = kv_bytes(model, context).saturating_mul(slots.max(1));
    let margin_bytes = (weights_bytes as f64 * WEIGHT_MARGIN) as u64;
    let required_bytes = weights_bytes
        .saturating_add(kv)
        .saturating_add(margin_bytes);
    let available_bytes: u64 = nodes.iter().map(usable_bytes).fold(0, u64::saturating_add);
    let count = nodes.len() as u32;
    let gateway_bytes = gateway_need(weights_bytes);
    let gateway_fits = gateway_free.is_none_or(|free| free >= gateway_bytes);
    let ok = count > 0 && available_bytes >= required_bytes && gateway_fits;
    let largest = nodes.iter().map(usable_bytes).max().unwrap_or(0);
    let survives_one_loss = count > 1 && available_bytes - largest >= required_bytes;
    let kv_note = if kv == 0 && (model.layers == 0 || model.hidden == 0 || model.heads == 0) {
        " (cache de contexto não estimado: o cabeçalho não traz as dimensões)"
    } else {
        ""
    };
    let message = if ok {
        format!(
            "cabe: precisa de {:.2} GB e {} {:.2} GB{}",
            gb(required_bytes),
            plural(count),
            gb(available_bytes),
            if survives_one_loss {
                ""
            } else {
                " (sem folga se um worker cair)"
            }
        )
    } else if count == 0 {
        "nenhum worker aprovado e vivo para receber o modelo".to_string()
    } else if available_bytes >= required_bytes {
        format!(
            "não cabe no gateway: o llama-server precisa de cerca de {:.2} GB e a máquina dele tem {:.2} GB livres",
            gb(gateway_bytes),
            gb(gateway_free.unwrap_or(0))
        )
    } else {
        let missing = required_bytes - available_bytes;
        let average = available_bytes / u64::from(count);
        let advice = if average < MIN_USEFUL_NODE {
            "os workers atuais quase não têm RAM utilizável; acrescente workers com mais RAM"
                .to_string()
        } else {
            format!(
                "acrescente cerca de {} workers como o médio atual ({:.2} GB cada)",
                missing.div_ceil(average),
                gb(average)
            )
        };
        format!(
            "não cabe: faltam {:.2} GB (precisa de {:.2} GB, {} {:.2} GB); {advice}",
            gb(missing),
            gb(required_bytes),
            plural(count),
            gb(available_bytes),
        )
    };
    let message = format!("{message}{kv_note}");
    Verdict {
        ok,
        weights_bytes,
        kv_bytes: kv,
        margin_bytes,
        required_bytes,
        available_bytes,
        nodes: count,
        survives_one_loss,
        gateway_bytes,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1 << 30;

    fn node(gib: u64) -> NodeMemory {
        NodeMemory {
            total_bytes: gib * GIB,
            used_fraction: 0.0,
        }
    }

    fn model(weights_gib: u64) -> ModelEntry {
        ModelEntry {
            bytes: weights_gib * GIB,
            supported: true,
            ..ModelEntry::default()
        }
    }

    #[test]
    fn a_node_offers_at_most_80_percent_minus_the_overhead() {
        // 4 GiB: 80% = 3,2 GiB, menos 128 MiB.
        let offered = usable_bytes(&node(4));
        assert_eq!(offered, (4.0 * GIB as f64 * 0.8) as u64 - (128 << 20));
        // Com 90% em uso, só 10% está livre.
        let busy = NodeMemory {
            total_bytes: 4 * GIB,
            used_fraction: 0.9,
        };
        assert!(usable_bytes(&busy) < GIB / 2);
    }

    #[test]
    fn it_fits_when_the_sum_of_the_nodes_covers_weights_and_margin() {
        let verdict = check(&model(8), 0, 1, &[node(4), node(4), node(4)], None);
        assert!(verdict.ok, "{}", verdict.message);
        assert_eq!(verdict.nodes, 3);
        assert!(verdict.required_bytes > 8 * GIB);
        assert!(verdict.message.starts_with("cabe"));
    }

    #[test]
    fn the_failover_margin_is_reported_but_does_not_decide() {
        let three = check(&model(8), 0, 1, &[node(4), node(4), node(4)], None);
        assert!(three.ok && !three.survives_one_loss);
        assert!(three.message.contains("sem folga"), "{}", three.message);
        let four: Vec<_> = (0..4).map(|_| node(4)).collect();
        let four = check(&model(8), 0, 1, &four, None);
        assert!(four.ok && four.survives_one_loss);
        assert!(!check(&model(1), 0, 1, &[node(8)], None).survives_one_loss);
    }

    #[test]
    fn the_gateway_ram_can_veto_a_model_the_workers_hold() {
        // 8 GiB de pesos pedem ~2,5 GiB no gateway (128 MiB + 30%).
        let need = gateway_need(8 * GIB);
        assert!(need > 2 * GIB && need < 3 * GIB);
        let nodes = [node(8), node(8)];
        assert!(
            check(&model(8), 0, 1, &nodes, None).ok,
            "sem informação não confere"
        );
        assert!(check(&model(8), 0, 1, &nodes, Some(need)).ok);
        let no = check(&model(8), 0, 1, &nodes, Some(GIB));
        assert!(!no.ok && no.message.contains("gateway"), "{}", no.message);
        assert_eq!(no.gateway_bytes, need);
    }

    #[test]
    fn it_says_how_much_is_missing_and_how_many_nodes_to_add() {
        let verdict = check(&model(16), 0, 1, &[node(4), node(4)], None);
        assert!(!verdict.ok);
        assert!(verdict.message.contains("faltam"), "{}", verdict.message);
        assert!(verdict.message.contains("acrescente cerca de"));
        // Mais workers do mesmo tamanho resolvem: o veredito deixa de ser NOK.
        let more: Vec<_> = (0..8).map(|_| node(4)).collect();
        assert!(check(&model(16), 0, 1, &more, None).ok);
    }

    #[test]
    fn nodes_without_usable_ram_do_not_produce_a_silly_node_count() {
        let starved = NodeMemory {
            total_bytes: 100 << 20,
            used_fraction: 0.0,
        };
        let verdict = check(&model(1), 0, 1, &[starved], None);
        assert!(!verdict.ok);
        assert!(
            verdict.message.contains("quase não têm RAM"),
            "{}",
            verdict.message
        );
    }

    #[test]
    fn hostile_numbers_never_panic_or_look_free() {
        let mut m = model(1);
        m.layers = u32::MAX;
        m.hidden = u32::MAX;
        m.heads = 1;
        m.kv_heads = u32::MAX; // mais cabeças kv que de consulta: limitado às de consulta
        m.context = u64::MAX;
        assert_eq!(kv_bytes(&m, u64::MAX), u64::MAX);
        // Duas memórias gigantes somam sem estouro.
        let huge = NodeMemory {
            total_bytes: u64::MAX,
            used_fraction: 0.0,
        };
        assert!(check(&model(1), 0, 1, &[huge.clone(), huge], None).ok);
        // Medida inválida: o worker conta como cheio.
        let nan = NodeMemory {
            total_bytes: 8 * GIB,
            used_fraction: f64::NAN,
        };
        assert_eq!(usable_bytes(&nan), 0);
    }

    #[test]
    fn a_missing_kv_estimate_is_said_out_loud() {
        let verdict = check(&model(1), 0, 1, &[node(8)], None);
        assert!(verdict.ok);
        assert!(
            verdict.message.contains("não estimado"),
            "{}",
            verdict.message
        );
    }

    #[test]
    fn no_node_is_a_nok_with_its_own_reason() {
        let verdict = check(&model(1), 0, 1, &[], None);
        assert!(!verdict.ok);
        assert!(verdict.message.contains("nenhum worker"));
    }

    #[test]
    fn the_kv_cache_follows_layers_context_and_grouped_heads() {
        let mut m = model(1);
        m.layers = 32;
        m.hidden = 4096;
        m.heads = 32;
        m.kv_heads = 8; // atenção agrupada: o cache é 4x menor
        m.context = 8192;
        // 2 (K e V) · 2 B · 32 camadas · 4096 tokens · (4096 · 8/32 = 1024)
        assert_eq!(kv_bytes(&m, 4096), 2 * 2 * 32 * 4096 * 1024);
        // Pede mais contexto do que o modelo tem: usa o do modelo.
        assert_eq!(kv_bytes(&m, 1 << 20), 2 * 2 * 32 * 8192 * 1024);
        // Sem dimensões no cabeçalho, não inventa.
        assert_eq!(kv_bytes(&model(1), 4096), 0);
    }

    #[test]
    fn each_slot_adds_its_own_kv_cache() {
        let mut m = model(1);
        m.layers = 32;
        m.hidden = 4096;
        m.heads = 32;
        m.kv_heads = 32;
        m.context = 4096;
        let one = check(&m, 4096, 1, &[node(8)], None).kv_bytes;
        assert_eq!(check(&m, 4096, 4, &[node(8)], None).kv_bytes, 4 * one);
        assert_eq!(check(&m, 4096, 0, &[node(8)], None).kv_bytes, one);
    }

    #[test]
    fn a_bigger_context_can_turn_an_ok_into_a_nok() {
        let mut m = model(4);
        m.layers = 32;
        m.hidden = 4096;
        m.heads = 32;
        m.kv_heads = 32;
        m.context = 131_072;
        let nodes = [node(4), node(4)];
        assert!(check(&m, 2048, 1, &nodes, None).ok);
        assert!(!check(&m, 131_072, 1, &nodes, None).ok);
    }
}
