use crate::tensor::DataLocation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferResidency {
    Local,
    Cached,
    Remote,
    Missing,
}

impl BufferResidency {
    pub const fn is_available(self) -> bool {
        !matches!(self, Self::Missing)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferAccessSource {
    Local,
    ControllerCache,
    Primary,
    Recompute,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferAccessDecision {
    pub source: Option<BufferAccessSource>,
    pub estimated_duration_us: Option<u64>,
}

impl BufferAccessDecision {
    pub fn unavailable() -> Self {
        Self {
            source: None,
            estimated_duration_us: None,
        }
    }

    pub const fn is_available(&self) -> bool {
        self.source.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BufferAccessOptions {
    pub local_copy_available: bool,
    pub controller_cache_available: bool,
    pub primary_available: bool,
    pub cache_transfer_duration_us: Option<u64>,
    pub primary_transfer_duration_us: Option<u64>,
    pub recompute_duration_us: Option<u64>,
}

pub fn classify_buffer_residency(
    primary_location: Option<&DataLocation>,
    local_copy_available: bool,
    controller_cache_available: bool,
    primary_available: bool,
) -> BufferResidency {
    if local_copy_available {
        BufferResidency::Local
    } else if controller_cache_available {
        BufferResidency::Cached
    } else if primary_location.is_some() && primary_available {
        BufferResidency::Remote
    } else {
        BufferResidency::Missing
    }
}

pub fn plan_buffer_access(options: BufferAccessOptions) -> BufferAccessDecision {
    if options.local_copy_available {
        return BufferAccessDecision {
            source: Some(BufferAccessSource::Local),
            estimated_duration_us: Some(0),
        };
    }

    let mut best: Option<(u8, BufferAccessSource, u64)> = None;

    let mut consider = |priority: u8, source: BufferAccessSource, duration: Option<u64>| {
        let Some(duration) = duration else {
            return;
        };

        match best {
            Some((best_priority, _, best_duration))
                if duration > best_duration
                    || (duration == best_duration && priority >= best_priority) => {}
            _ => best = Some((priority, source, duration)),
        }
    };

    if options.controller_cache_available {
        consider(
            0,
            BufferAccessSource::ControllerCache,
            options.cache_transfer_duration_us,
        );
    }

    if options.primary_available {
        consider(
            1,
            BufferAccessSource::Primary,
            options.primary_transfer_duration_us,
        );
    }

    consider(
        2,
        BufferAccessSource::Recompute,
        options.recompute_duration_us,
    );

    best.map_or_else(
        BufferAccessDecision::unavailable,
        |(_, source, duration)| BufferAccessDecision {
            source: Some(source),
            estimated_duration_us: Some(duration),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{
        classify_buffer_residency, plan_buffer_access, BufferAccessOptions, BufferAccessSource,
        BufferResidency,
    };
    use crate::tensor::DataLocation;

    fn primary() -> DataLocation {
        DataLocation::Node {
            node_id: "node-a".into(),
        }
    }

    #[test]
    fn localidade_prioriza_copia_local() {
        assert_eq!(
            classify_buffer_residency(Some(&primary()), true, true, true),
            BufferResidency::Local
        );
    }

    #[test]
    fn cache_eh_identificado_quando_nao_ha_copia_local() {
        assert_eq!(
            classify_buffer_residency(Some(&primary()), false, true, true),
            BufferResidency::Cached
        );
    }

    #[test]
    fn primary_disponivel_sem_cache_eh_remoto() {
        assert_eq!(
            classify_buffer_residency(Some(&primary()), false, false, true),
            BufferResidency::Remote
        );
    }

    #[test]
    fn ausencia_de_fontes_eh_missing() {
        assert_eq!(
            classify_buffer_residency(Some(&primary()), false, false, false),
            BufferResidency::Missing
        );
        assert!(!BufferResidency::Missing.is_available());
    }

    #[test]
    fn reuso_local_tem_custo_zero() {
        let decision = plan_buffer_access(BufferAccessOptions {
            local_copy_available: true,
            controller_cache_available: true,
            primary_available: true,
            cache_transfer_duration_us: Some(10),
            primary_transfer_duration_us: Some(20),
            recompute_duration_us: Some(5),
        });

        assert_eq!(decision.source, Some(BufferAccessSource::Local));
        assert_eq!(decision.estimated_duration_us, Some(0));
    }

    #[test]
    fn escolhe_cache_quando_eh_o_caminho_mais_barato() {
        let decision = plan_buffer_access(BufferAccessOptions {
            local_copy_available: false,
            controller_cache_available: true,
            primary_available: true,
            cache_transfer_duration_us: Some(10),
            primary_transfer_duration_us: Some(20),
            recompute_duration_us: Some(30),
        });

        assert_eq!(decision.source, Some(BufferAccessSource::ControllerCache));
        assert_eq!(decision.estimated_duration_us, Some(10));
    }

    #[test]
    fn escolhe_transferencia_primary_quando_cache_eh_pior() {
        let decision = plan_buffer_access(BufferAccessOptions {
            local_copy_available: false,
            controller_cache_available: true,
            primary_available: true,
            cache_transfer_duration_us: Some(30),
            primary_transfer_duration_us: Some(10),
            recompute_duration_us: Some(20),
        });

        assert_eq!(decision.source, Some(BufferAccessSource::Primary));
        assert_eq!(decision.estimated_duration_us, Some(10));
    }

    #[test]
    fn escolhe_recomputacao_quando_eh_mais_barata() {
        let decision = plan_buffer_access(BufferAccessOptions {
            local_copy_available: false,
            controller_cache_available: true,
            primary_available: true,
            cache_transfer_duration_us: Some(30),
            primary_transfer_duration_us: Some(20),
            recompute_duration_us: Some(10),
        });

        assert_eq!(decision.source, Some(BufferAccessSource::Recompute));
        assert_eq!(decision.estimated_duration_us, Some(10));
    }

    #[test]
    fn empate_prefere_cache_depois_primary_depois_recomputacao() {
        let decision = plan_buffer_access(BufferAccessOptions {
            local_copy_available: false,
            controller_cache_available: true,
            primary_available: true,
            cache_transfer_duration_us: Some(10),
            primary_transfer_duration_us: Some(10),
            recompute_duration_us: Some(10),
        });

        assert_eq!(decision.source, Some(BufferAccessSource::ControllerCache));
        assert_eq!(decision.estimated_duration_us, Some(10));
    }

    #[test]
    fn sem_caminhos_disponiveis_resulta_em_unavailable() {
        let decision = plan_buffer_access(BufferAccessOptions {
            local_copy_available: false,
            controller_cache_available: false,
            primary_available: false,
            cache_transfer_duration_us: None,
            primary_transfer_duration_us: None,
            recompute_duration_us: None,
        });

        assert_eq!(decision.source, None);
        assert_eq!(decision.estimated_duration_us, None);
        assert!(!decision.is_available());
    }
}
