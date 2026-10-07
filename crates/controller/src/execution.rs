use std::{
    collections::HashMap,
    fmt::{Display, Formatter},
};

use real_ai_cluster_protocol::v1::{NodeInfo, NodeState, WorkUnit};

use crate::scheduler;

#[derive(Debug, Clone)]
pub struct WorkUnitSpec {
    pub work: WorkUnit,
    pub depends_on: Vec<String>,
    pub data_from: Vec<String>,
    pub required_backends: Vec<String>,
}

impl WorkUnitSpec {
    pub fn new(work: WorkUnit) -> Self {
        Self {
            work,
            depends_on: Vec::new(),
            data_from: Vec::new(),
            required_backends: Vec::new(),
        }
    }

    pub fn with_dependencies(mut self, depends_on: Vec<String>) -> Self {
        self.depends_on = depends_on;
        self
    }

    pub fn with_data_inputs(mut self, data_from: Vec<String>) -> Self {
        self.data_from = data_from;
        self
    }

    pub fn with_required_backends(mut self, required_backends: Vec<String>) -> Self {
        self.required_backends = required_backends;
        self
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionGraph {
    pub execution_id: String,
    pub units: Vec<WorkUnitSpec>,
}

impl ExecutionGraph {
    pub fn new(execution_id: impl Into<String>) -> Self {
        Self {
            execution_id: execution_id.into(),
            units: Vec::new(),
        }
    }

    pub fn single(work: WorkUnit) -> Self {
        let execution_id = work.work_id.clone();
        let mut graph = Self::new(execution_id);
        graph.units.push(WorkUnitSpec::new(work));
        graph
    }

    pub fn add_unit(&mut self, unit: WorkUnitSpec) -> Result<(), PlanError> {
        if self
            .units
            .iter()
            .any(|existing| existing.work.work_id == unit.work.work_id)
        {
            return Err(PlanError::DuplicateWorkUnit(unit.work.work_id));
        }

        self.units.push(unit);
        Ok(())
    }

    pub fn levels(&self) -> Result<Vec<Vec<String>>, PlanError> {
        let mut index = HashMap::with_capacity(self.units.len());

        for (position, unit) in self.units.iter().enumerate() {
            if index.insert(unit.work.work_id.as_str(), position).is_some() {
                return Err(PlanError::DuplicateWorkUnit(unit.work.work_id.clone()));
            }
        }

        let mut indegree = vec![0usize; self.units.len()];
        let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); self.units.len()];

        for (position, unit) in self.units.iter().enumerate() {
            let mut dependencies = std::collections::HashSet::new();
            dependencies.extend(unit.depends_on.iter().map(String::as_str));
            dependencies.extend(unit.data_from.iter().map(String::as_str));

            for dependency in dependencies {
                let dependency_position =
                    index.get(dependency).copied().ok_or_else(|| {
                        PlanError::MissingDependency {
                            work_unit_id: unit.work.work_id.clone(),
                            dependency: dependency.to_owned(),
                        }
                    })?;

                indegree[position] += 1;
                dependents[dependency_position].push(position);
            }
        }

        let mut emitted = vec![false; self.units.len()];
        let mut levels = Vec::new();

        loop {
            let current: Vec<usize> = indegree
                .iter()
                .enumerate()
                .filter_map(|(position, degree)| {
                    if *degree == 0 && !emitted[position] {
                        Some(position)
                    } else {
                        None
                    }
                })
                .collect();

            if current.is_empty() {
                break;
            }

            let mut level = Vec::with_capacity(current.len());

            for position in current {
                emitted[position] = true;
                level.push(self.units[position].work.work_id.clone());

                for dependent in &dependents[position] {
                    indegree[*dependent] = indegree[*dependent].saturating_sub(1);
                }
            }

            levels.push(level);
        }

        if emitted.iter().all(|value| *value) {
            Ok(levels)
        } else {
            Err(PlanError::Cycle)
        }
    }

    fn unit(&self, work_unit_id: &str) -> Result<&WorkUnitSpec, PlanError> {
        self.units
            .iter()
            .find(|unit| unit.work.work_id == work_unit_id)
            .ok_or_else(|| PlanError::UnknownWorkUnit(work_unit_id.to_owned()))
    }
}

#[derive(Debug, Clone)]
pub struct PlacementAssignment {
    pub work_unit_id: String,
    pub node_id: String,
    pub rank: usize,
    pub score: f64,
}

#[derive(Debug, Clone)]
pub struct PlacementPlan {
    pub execution_id: String,
    pub assignments: Vec<PlacementAssignment>,
}

impl PlacementPlan {
    pub fn node_for(&self, work_unit_id: &str) -> Option<&PlacementAssignment> {
        self.assignments
            .iter()
            .find(|assignment| assignment.work_unit_id == work_unit_id)
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct PlacementPlanner;

impl PlacementPlanner {
    pub fn plan(
        &self,
        graph: &ExecutionGraph,
        nodes: &[NodeInfo],
    ) -> Result<PlacementPlan, PlanError> {
        let levels = graph.levels()?;

        let candidates: Vec<NodeInfo> = nodes
            .iter()
            .filter(|node| {
                matches!(
                    NodeState::try_from(node.state),
                    Ok(NodeState::Ready | NodeState::Active)
                ) && !node.worker_endpoint.is_empty()
            })
            .cloned()
            .collect();

        if candidates.is_empty() {
            return Err(PlanError::NoSchedulableNodes);
        }

        let mut assignments = Vec::with_capacity(graph.units.len());

        for level in levels {
            let mut used_nodes = std::collections::HashSet::new();

            for work_unit_id in level {
                let unit = graph.unit(&work_unit_id)?;

                let eligible: Vec<NodeInfo> = candidates
                    .iter()
                    .filter(|node| backend_compatible(node, &unit.required_backends))
                    .cloned()
                    .collect();

                if eligible.is_empty() {
                    return Err(PlanError::NoCompatibleNode {
                        work_unit_id: work_unit_id.clone(),
                    });
                }

                let ranked = scheduler::rank_workers_with_scores(eligible);
                let selected = ranked
                    .iter()
                    .enumerate()
                    .find(|(_, worker)| !used_nodes.contains(&worker.node.node_id))
                    .or_else(|| ranked.iter().enumerate().next())
                    .expect("eligible list is not empty");

                used_nodes.insert(selected.1.node.node_id.clone());

                assignments.push(PlacementAssignment {
                    work_unit_id,
                    node_id: selected.1.node.node_id.clone(),
                    rank: selected.0 + 1,
                    score: selected.1.score,
                });
            }
        }

        Ok(PlacementPlan {
            execution_id: graph.execution_id.clone(),
            assignments,
        })
    }
}

fn backend_compatible(node: &NodeInfo, required_backends: &[String]) -> bool {
    if required_backends.is_empty() {
        return true;
    }

    node.capability.as_ref().is_some_and(|capability| {
        required_backends.iter().all(|required| {
            capability
                .execution_backends
                .iter()
                .any(|backend| backend == required)
        })
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    DuplicateWorkUnit(String),
    MissingDependency {
        work_unit_id: String,
        dependency: String,
    },
    UnknownWorkUnit(String),
    Cycle,
    NoSchedulableNodes,
    NoCompatibleNode {
        work_unit_id: String,
    },
}

impl Display for PlanError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateWorkUnit(work_unit_id) => {
                write!(formatter, "work unit duplicada: {work_unit_id}")
            }
            Self::MissingDependency {
                work_unit_id,
                dependency,
            } => write!(
                formatter,
                "dependência inexistente: {dependency} em {work_unit_id}"
            ),
            Self::UnknownWorkUnit(work_unit_id) => {
                write!(formatter, "work unit desconhecida: {work_unit_id}")
            }
            Self::Cycle => write!(formatter, "execution graph contém ciclo"),
            Self::NoSchedulableNodes => write!(formatter, "nenhum node elegível para placement"),
            Self::NoCompatibleNode { work_unit_id } => write!(
                formatter,
                "nenhum node compatível com a work unit: {work_unit_id}"
            ),
        }
    }
}

impl std::error::Error for PlanError {}

#[cfg(test)]
mod tests {
    use super::{ExecutionGraph, PlacementPlanner, PlanError, WorkUnitSpec};
    use real_ai_cluster_protocol::v1::{Capability, NodeInfo, NodeState, WorkUnit};

    fn work(work_id: &str, operation: &str) -> WorkUnit {
        WorkUnit {
            work_id: work_id.into(),
            operation: operation.into(),
            ..Default::default()
        }
    }

    fn node(node_id: &str, backends: &[&str], active_work_units: u32) -> NodeInfo {
        NodeInfo {
            node_id: node_id.into(),
            state: NodeState::Ready as i32,
            worker_endpoint: format!("http://{node_id}:50052"),
            capability: Some(Capability {
                execution_backends: backends.iter().map(|value| (*value).into()).collect(),
                cpu_logical_cores: 4,
                cpu_ops_per_sec: 100.0,
                network_rtt_us: 100.0,
                network_mb_per_sec: 100.0,
                ..Default::default()
            }),
            active_work_units,
            ..Default::default()
        }
    }

    #[test]
    fn execution_graph_detecta_dependencia_inexistente() {
        let mut graph = ExecutionGraph::new("exec-1");
        graph
            .add_unit(WorkUnitSpec::new(work("w2", "echo")).with_dependencies(vec!["w1".into()]))
            .unwrap();

        assert!(matches!(
            graph.levels(),
            Err(PlanError::MissingDependency { .. })
        ));
    }

    #[test]
    fn execution_graph_data_from_cria_dependencia_de_execucao() {
        let mut graph = ExecutionGraph::new("exec-1");
        graph
            .add_unit(WorkUnitSpec::new(work("w1", "echo")))
            .unwrap();
        graph
            .add_unit(WorkUnitSpec::new(work("w2", "echo")).with_data_inputs(vec!["w1".into()]))
            .unwrap();

        assert_eq!(
            graph.levels().unwrap(),
            vec![vec!["w1".to_string()], vec!["w2".to_string()]]
        );
    }

    #[test]
    fn execution_graph_detecta_ciclo() {
        let mut graph = ExecutionGraph::new("exec-1");
        graph
            .add_unit(WorkUnitSpec::new(work("w1", "echo")).with_dependencies(vec!["w2".into()]))
            .unwrap();
        graph
            .add_unit(WorkUnitSpec::new(work("w2", "echo")).with_dependencies(vec!["w1".into()]))
            .unwrap();

        assert_eq!(graph.levels(), Err(PlanError::Cycle));
    }

    #[test]
    fn execution_graph_calcula_niveis_de_paralelismo() {
        let mut graph = ExecutionGraph::new("exec-1");
        graph
            .add_unit(WorkUnitSpec::new(work("w1", "echo")))
            .unwrap();
        graph
            .add_unit(WorkUnitSpec::new(work("w2", "echo")))
            .unwrap();
        graph
            .add_unit(
                WorkUnitSpec::new(work("w3", "echo"))
                    .with_dependencies(vec!["w1".into(), "w2".into()]),
            )
            .unwrap();

        assert_eq!(
            graph.levels().unwrap(),
            vec![
                vec!["w1".to_string(), "w2".to_string()],
                vec!["w3".to_string()]
            ]
        );
    }

    #[test]
    fn placement_distribui_unidades_independentes() {
        let mut graph = ExecutionGraph::new("exec-1");
        graph
            .add_unit(WorkUnitSpec::new(work("w1", "echo")))
            .unwrap();
        graph
            .add_unit(WorkUnitSpec::new(work("w2", "echo")))
            .unwrap();

        let nodes = vec![node("node-a", &["cpu"], 0), node("node-b", &["cpu"], 0)];

        let plan = PlacementPlanner::default().plan(&graph, &nodes).unwrap();

        assert_eq!(plan.node_for("w1").unwrap().node_id, "node-a");
        assert_eq!(plan.node_for("w2").unwrap().node_id, "node-b");
    }

    #[test]
    fn placement_filtra_backend_incompativel() {
        let mut graph = ExecutionGraph::new("exec-1");
        graph
            .add_unit(
                WorkUnitSpec::new(work("w1", "echo")).with_required_backends(vec!["cuda".into()]),
            )
            .unwrap();

        let nodes = vec![
            node("cpu-node", &["cpu"], 0),
            node("cuda-node", &["cpu", "cuda"], 0),
        ];

        let plan = PlacementPlanner::default().plan(&graph, &nodes).unwrap();

        assert_eq!(plan.node_for("w1").unwrap().node_id, "cuda-node");
    }

    #[test]
    fn placement_rejeita_quando_nao_ha_backend_compativel() {
        let mut graph = ExecutionGraph::new("exec-1");
        graph
            .add_unit(
                WorkUnitSpec::new(work("w1", "echo")).with_required_backends(vec!["cuda".into()]),
            )
            .unwrap();

        let nodes = vec![node("cpu-node", &["cpu"], 0)];

        assert!(matches!(
            PlacementPlanner::default().plan(&graph, &nodes),
            Err(PlanError::NoCompatibleNode { .. })
        ));
    }
}
